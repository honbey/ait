use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::app::AppState;
use crate::db::{ApiKeyUpdate, AuditEvent, RequestId};
use crate::error::{AitError, internal_error};
use crate::handlers::{ident_chars, validate_string};

#[derive(Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
    pub expires_at: Option<i64>,
}

#[derive(serde::Serialize)]
pub struct ApiKeyResponse {
    pub id: String,
    pub key: String,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub enabled: bool,
    pub expires_at: Option<i64>,
}

pub async fn create_api_key(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(input): Json<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<ApiKeyResponse>), (StatusCode, Json<AitError>)> {
    let expires_at: Option<DateTime<Utc>> = input
        .expires_at
        .filter(|&ts| ts != 0)
        .map(|ts| {
            DateTime::from_timestamp(ts, 0)
                .ok_or_else(|| AitError::bad_request("Invalid expires_at").into_response())
        })
        .transpose()?;

    let name = validate_string(&input.name, "name", 128, ident_chars)?;
    let db = state.db.clone();
    let (stored, raw_key) = crate::run_blocking(move || db.insert_api_key(&name, expires_at))
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        request_id: request_id.0,
        action: "create".into(),
        resource: "api_key".into(),
        resource_id: stored.id.clone(),
        detail: None,
    });

    Ok((
        StatusCode::CREATED,
        Json(ApiKeyResponse {
            id: stored.id.clone(),
            key: raw_key,
            name: stored.name,
            created_at: stored.created_at.timestamp(),
            updated_at: stored.updated_at.timestamp(),
            enabled: stored.enabled,
            expires_at: stored.expires_at.map(|dt| dt.timestamp()),
        }),
    ))
}

pub async fn list_api_keys(
    State(state): State<AppState>,
) -> Result<Json<Vec<ApiKeyResponse>>, (StatusCode, Json<AitError>)> {
    let db = state.db.clone();
    let keys = crate::run_blocking(move || db.list_api_keys())
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;

    let items: Vec<ApiKeyResponse> = keys
        .into_iter()
        .map(|k| ApiKeyResponse {
            id: k.id.clone(),
            key: k.masked(),
            name: k.name,
            created_at: k.created_at.timestamp(),
            updated_at: k.updated_at.timestamp(),
            enabled: k.enabled,
            expires_at: k.expires_at.map(|dt| dt.timestamp()),
        })
        .collect();
    Ok(Json(items))
}

pub async fn delete_api_key(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(key): Path<String>,
) -> Result<(StatusCode,), (StatusCode, Json<AitError>)> {
    let db = state.db.clone();
    let key_clone = key.clone();
    let hash = crate::run_blocking(move || db.delete_api_key(&key_clone))
        .await
        .map_err(internal_error)?
        .map_err(|e| AitError::from_db_error(e).into_response())?;
    state.api_key_cache.remove(&hash);

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        request_id: request_id.0,
        action: "delete".into(),
        resource: "api_key".into(),
        resource_id: key,
        detail: None,
    });

    Ok((StatusCode::NO_CONTENT,))
}

#[derive(Deserialize)]
pub struct UpdateApiKeyRequest {
    pub name: Option<String>,
    pub expires_at: Option<i64>,
    pub enabled: Option<bool>,
}

pub async fn update_api_key(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(key_id): Path<String>,
    Json(input): Json<UpdateApiKeyRequest>,
) -> Result<Json<ApiKeyResponse>, (StatusCode, Json<AitError>)> {
    let name = match input.name {
        Some(n) => {
            let t = n.trim().to_string();
            if t.is_empty() {
                None
            } else {
                Some(validate_string(&t, "name", 128, ident_chars)?)
            }
        }
        None => None,
    };
    let updates = ApiKeyUpdate {
        id: key_id.clone(),
        name,
        enabled: input.enabled,
        expires_at: input
            .expires_at
            .map(|ts| {
                if ts == 0 {
                    return Ok(DateTime::UNIX_EPOCH);
                }
                DateTime::from_timestamp(ts, 0)
                    .ok_or_else(|| AitError::bad_request("Invalid expires_at").into_response())
            })
            .transpose()?,
    };
    let db = state.db.clone();
    let (updated, hash) = crate::run_blocking(move || db.update_api_key(&updates))
        .await
        .map_err(internal_error)?
        .map_err(|e| AitError::from_db_error(e).into_response())?;
    state.api_key_cache.remove(&hash);

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        request_id: request_id.0,
        action: "update".into(),
        resource: "api_key".into(),
        resource_id: key_id,
        detail: None,
    });

    Ok(Json(ApiKeyResponse {
        id: updated.id.clone(),
        key: updated.masked(),
        name: updated.name,
        created_at: updated.created_at.timestamp(),
        updated_at: updated.updated_at.timestamp(),
        enabled: updated.enabled,
        expires_at: updated.expires_at.map(|dt| dt.timestamp()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{create_test_state, test_router};
    use axum::Router;
    use axum::http::Method;
    use tower::ServiceExt;

    async fn create_key(router: &Router, name: &str) -> serde_json::Value {
        let resp = send_request(
            router,
            Method::POST,
            "/api/api-keys",
            None,
            Some(serde_json::json!({"name": name})),
        )
        .await;
        assert_eq!(
            resp.status,
            StatusCode::CREATED,
            "create key should succeed"
        );
        resp.json
    }

    async fn send_request(
        router: &Router,
        method: Method,
        uri: &str,
        bearer: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> TestResponse {
        let mut builder = axum::http::Request::builder().method(method).uri(uri);
        if let Some(bearer) = bearer {
            builder = builder.header(
                axum::http::header::AUTHORIZATION,
                format!("Bearer {bearer}"),
            );
        }
        if body.is_some() {
            builder = builder.header(axum::http::header::CONTENT_TYPE, "application/json");
        }
        let body = body.map(|b| b.to_string()).unwrap_or_default();
        let mut request = builder.body(axum::body::Body::from(body)).unwrap();
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
                [127, 0, 0, 1],
                0,
            ))));
        let response = router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
        };
        TestResponse {
            status,
            json,
            headers,
        }
    }

    struct TestResponse {
        status: StatusCode,
        json: serde_json::Value,
        headers: axum::http::HeaderMap,
    }

    #[tokio::test]
    async fn create_api_key_returns_raw_key() {
        let (state, _dir) = create_test_state();
        let router = test_router(state);
        let json = create_key(&router, "test-key").await;
        assert_eq!(json["name"], "test-key");
        assert_eq!(json["enabled"], serde_json::Value::Bool(true));
        let key = json["key"].as_str().expect("raw key should be returned");
        assert!(!key.is_empty());
        assert!(!key.contains('*'), "create should return the raw key");
    }

    #[tokio::test]
    async fn create_api_key_empty_name_bad_request() {
        let (state, _dir) = create_test_state();
        let router = test_router(state);
        let resp = send_request(
            &router,
            Method::POST,
            "/api/api-keys",
            None,
            Some(serde_json::json!({"name": "   "})),
        )
        .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        assert_eq!(resp.json["code"], 400);
    }

    #[tokio::test]
    async fn update_api_key_disabled_key_rejected_by_proxy() {
        let (state, _dir) = create_test_state();
        let router = test_router(state);
        let json = create_key(&router, "test-key").await;
        let key_id = json["id"].as_str().unwrap();
        let raw_key = json["key"].as_str().unwrap();

        let resp = send_request(
            &router,
            Method::PUT,
            &format!("/api/api-keys/{key_id}"),
            None,
            Some(serde_json::json!({"enabled": false})),
        )
        .await;
        assert_eq!(resp.status, StatusCode::OK);
        assert_eq!(resp.json["enabled"], serde_json::Value::Bool(false));

        let resp = send_request(&router, Method::GET, "/v1/models", Some(raw_key), None).await;
        assert_eq!(resp.status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn delete_api_key_removes_key() {
        let (state, _dir) = create_test_state();
        let router = test_router(state);
        let json = create_key(&router, "test-key").await;
        let key_id = json["id"].as_str().unwrap();
        let raw_key = json["key"].as_str().unwrap();

        let resp = send_request(
            &router,
            Method::DELETE,
            &format!("/api/api-keys/{key_id}"),
            None,
            None,
        )
        .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = send_request(&router, Method::GET, "/v1/models", Some(raw_key), None).await;
        assert_eq!(resp.status, StatusCode::UNAUTHORIZED);
    }
}
