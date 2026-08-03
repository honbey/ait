use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::app::AppState;
use crate::db::{ApiKeyUpdate, AuditEvent, RequestId, SessionUser};
use crate::error::{AitError, forbidden, internal_error, not_found};
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
    Extension(session): Extension<SessionUser>,
    Extension(request_id): Extension<RequestId>,
    Path(username): Path<String>,
    Json(input): Json<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<ApiKeyResponse>), (StatusCode, Json<AitError>)> {
    if session.username != username {
        return Err(forbidden("You can only manage your own API keys"));
    }

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
    let username_clone = username.clone();
    let (stored, raw_key) =
        crate::run_blocking(move || db.insert_api_key(&username_clone, &name, expires_at))
            .await
            .map_err(internal_error)?
            .map_err(internal_error)?;

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        request_id: request_id.0,
        username: session.username.clone(),
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
    Extension(session): Extension<SessionUser>,
    Path(username): Path<String>,
) -> Result<Json<Vec<ApiKeyResponse>>, (StatusCode, Json<AitError>)> {
    if session.username != username {
        return Err(forbidden("You can only view your own API keys"));
    }

    let db = state.db.clone();
    let username_clone = username.clone();
    let user = crate::run_blocking(move || db.get_user(&username_clone))
        .await
        .map_err(internal_error)?
        .map_err(|e| AitError::from_db_error(e).into_response())?
        .ok_or_else(|| not_found(format!("User '{}' not found", username)))?;

    let items: Vec<ApiKeyResponse> = user
        .api_keys
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
    Extension(session): Extension<SessionUser>,
    Extension(request_id): Extension<RequestId>,
    Path((username, key)): Path<(String, String)>,
) -> Result<(StatusCode,), (StatusCode, Json<AitError>)> {
    if session.username != username {
        return Err(forbidden("You can only manage your own API keys"));
    }

    let db = state.db.clone();
    let username_clone = username.clone();
    let key_clone = key.clone();
    let hash = crate::run_blocking(move || db.delete_api_key(&username_clone, &key_clone))
        .await
        .map_err(internal_error)?
        .map_err(|e| AitError::from_db_error(e).into_response())?;
    state.api_key_cache.remove(&hash);

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        request_id: request_id.0,
        username: session.username.clone(),
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
    Extension(session): Extension<SessionUser>,
    Extension(request_id): Extension<RequestId>,
    Path((username, key_id)): Path<(String, String)>,
    Json(input): Json<UpdateApiKeyRequest>,
) -> Result<Json<ApiKeyResponse>, (StatusCode, Json<AitError>)> {
    if session.username != username {
        return Err(forbidden("You can only manage your own API keys"));
    }

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
    let username_clone = username.clone();
    let (updated, hash) = crate::run_blocking(move || db.update_api_key(&username_clone, &updates))
        .await
        .map_err(internal_error)?
        .map_err(|e| AitError::from_db_error(e).into_response())?;
    state.api_key_cache.remove(&hash);

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        request_id: request_id.0,
        username: session.username.clone(),
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
    use crate::test_utils::{
        create_test_state, insert_test_user, login_and_cookie, send_request, test_router,
    };
    use axum::Router;
    use axum::http::Method;

    async fn setup() -> (Router, String) {
        let (state, _dir) = create_test_state();
        insert_test_user(&state.db, "alice", "secret123");
        let router = test_router(state);
        let cookie = login_and_cookie(&router, "alice", "secret123").await;
        (router, cookie)
    }

    async fn create_key(router: &Router, cookie: &str, name: &str) -> serde_json::Value {
        let resp = send_request(
            router,
            Method::POST,
            "/api/users/alice/api-keys",
            Some(cookie),
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

    #[tokio::test]
    async fn create_api_key_returns_raw_key() {
        let (router, cookie) = setup().await;
        let json = create_key(&router, &cookie, "test-key").await;
        assert_eq!(json["name"], "test-key");
        assert_eq!(json["enabled"], serde_json::Value::Bool(true));
        let key = json["key"].as_str().expect("raw key should be returned");
        assert!(!key.is_empty());
        assert!(!key.contains('*'), "create should return the raw key");
    }

    #[tokio::test]
    async fn create_api_key_empty_name_bad_request() {
        let (router, cookie) = setup().await;
        let resp = send_request(
            &router,
            Method::POST,
            "/api/users/alice/api-keys",
            Some(&cookie),
            None,
            Some(serde_json::json!({"name": "   "})),
        )
        .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        assert_eq!(resp.json["code"], 400);
    }

    #[tokio::test]
    async fn create_api_key_cross_user_forbidden() {
        let (state, _dir) = create_test_state();
        insert_test_user(&state.db, "alice", "secret123");
        insert_test_user(&state.db, "bob", "bobpass");
        let router = test_router(state);
        let cookie = login_and_cookie(&router, "alice", "secret123").await;
        let resp = send_request(
            &router,
            Method::POST,
            "/api/users/bob/api-keys",
            Some(&cookie),
            None,
            Some(serde_json::json!({"name": "test-key"})),
        )
        .await;
        assert_eq!(resp.status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn list_api_keys_returns_created() {
        let (router, cookie) = setup().await;
        create_key(&router, &cookie, "test-key").await;
        let resp = send_request(
            &router,
            Method::GET,
            "/api/users/alice/api-keys",
            Some(&cookie),
            None,
            None,
        )
        .await;
        assert_eq!(resp.status, StatusCode::OK);
        let keys = resp.json.as_array().expect("list should return an array");
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0]["name"], "test-key");
        assert_eq!(keys[0]["enabled"], serde_json::Value::Bool(true));
    }

    #[tokio::test]
    async fn update_api_key_disabled_key_rejected_by_proxy() {
        let (router, cookie) = setup().await;
        let json = create_key(&router, &cookie, "test-key").await;
        let key_id = json["id"].as_str().unwrap();
        let raw_key = json["key"].as_str().unwrap();

        let resp = send_request(
            &router,
            Method::PUT,
            &format!("/api/users/alice/api-keys/{key_id}"),
            Some(&cookie),
            None,
            Some(serde_json::json!({"enabled": false})),
        )
        .await;
        assert_eq!(resp.status, StatusCode::OK);
        assert_eq!(resp.json["enabled"], serde_json::Value::Bool(false));

        // The disabled key must no longer pass the proxy auth middleware.
        let resp = send_request(
            &router,
            Method::GET,
            "/v1/models",
            None,
            Some(raw_key),
            None,
        )
        .await;
        assert_eq!(resp.status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn delete_api_key_removes_key() {
        let (router, cookie) = setup().await;
        let json = create_key(&router, &cookie, "test-key").await;
        let key_id = json["id"].as_str().unwrap();
        let raw_key = json["key"].as_str().unwrap();

        let resp = send_request(
            &router,
            Method::DELETE,
            &format!("/api/users/alice/api-keys/{key_id}"),
            Some(&cookie),
            None,
            None,
        )
        .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = send_request(
            &router,
            Method::GET,
            "/api/users/alice/api-keys",
            Some(&cookie),
            None,
            None,
        )
        .await;
        assert_eq!(resp.json.as_array().unwrap().len(), 0);

        let resp = send_request(
            &router,
            Method::GET,
            "/v1/models",
            None,
            Some(raw_key),
            None,
        )
        .await;
        assert_eq!(resp.status, StatusCode::UNAUTHORIZED);
    }
}
