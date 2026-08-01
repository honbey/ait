use std::sync::OnceLock;

use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::serde::ts_seconds;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use strum::{EnumMessage, IntoEnumIterator};

use crate::app::AppState;
use crate::db::{AuditEvent, Provider, ProviderType, ProviderUpdate, RequestId, SessionUser};
use crate::error::{AitError, internal_error, not_found};
use crate::handlers::{ident_chars, validate_string};
use crate::ssrf;

// ── Provider types ──

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ProviderResponse {
    id: String,
    name: String,
    #[serde(rename = "type")]
    provider_type: ProviderType,
    base_url: String,
    api_key: Option<String>,
    enabled: bool,
    #[serde(with = "ts_seconds")]
    created_at: DateTime<Utc>,
    #[serde(with = "ts_seconds")]
    updated_at: DateTime<Utc>,
}

impl From<Provider> for ProviderResponse {
    fn from(p: Provider) -> Self {
        Self {
            id: p.id,
            name: p.name,
            provider_type: p.provider_type,
            base_url: p.base_url,
            api_key: p
                .api_key
                .as_ref()
                .map(|k| crate::db::models::mask_api_key(k)),
            enabled: p.enabled,
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

#[derive(Deserialize)]
pub struct CreateProviderRequest {
    pub name: String,
    #[serde(rename = "type")]
    pub provider_type: ProviderType,
    pub base_url: String,
    pub api_key: Option<String>,
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct UpdateProviderRequest {
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub provider_type: Option<ProviderType>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub enabled: Option<bool>,
}

impl From<CreateProviderRequest> for Provider {
    fn from(r: CreateProviderRequest) -> Self {
        Provider {
            id: String::new(),
            name: r.name,
            provider_type: r.provider_type,
            base_url: r.base_url,
            api_key: r.api_key,
            enabled: r.enabled,
            created_at: chrono::DateTime::default(),
            updated_at: chrono::DateTime::default(),
        }
    }
}

// ── Validation ──

fn validate_base_url(url: &str) -> Result<reqwest::Url, AitError> {
    if !url
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | '~' | ':' | '/'))
    {
        return Err(AitError::bad_request(
            "base_url contains invalid characters",
        ));
    }
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| AitError::bad_request("base_url is not a valid URL"))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(AitError::bad_request(
            "base_url must use http:// or https:// scheme",
        ));
    }
    if parsed.host_str().is_none() {
        return Err(AitError::bad_request("base_url must include a host"));
    }
    let host_start = url.find("://").map(|i| i + 3).unwrap_or(0);
    let host_end = url[host_start..]
        .find(['/', ':'])
        .unwrap_or(url.len() - host_start);
    let host = &url[host_start..host_start + host_end];
    if !host.is_empty() && host.chars().all(|c| c.is_ascii_digit()) {
        return Err(AitError::bad_request(
            "base_url host cannot be purely numeric",
        ));
    }
    Ok(parsed)
}

// ── Handlers ──

pub async fn create_provider(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Extension(request_id): Extension<RequestId>,
    Json(input): Json<CreateProviderRequest>,
) -> Result<(StatusCode, Json<ProviderResponse>), (StatusCode, Json<AitError>)> {
    let name = validate_string(&input.name, "name", 128, ident_chars)?;
    let parsed_url =
        validate_base_url(&validate_string(&input.base_url, "base_url", 1024, |_| {
            true
        })?)?;
    ssrf::check_ssrf_config(
        &parsed_url,
        &state.config.security.ssrf_allowed_cidrs,
        &state.ssrf_dns_cache,
        &input.name,
    )
    .await
    .map_err(|e| e.into_response())?;
    let api_key = input.api_key.and_then(|k| {
        let t = k.trim().to_string();
        if t.is_empty() { None } else { Some(t) }
    });
    if let Some(ref k) = api_key
        && k.len() > 512
    {
        return Err(
            AitError::bad_request("api_key must not exceed 512 characters").into_response(),
        );
    }
    let provider = Provider {
        id: String::new(),
        name,
        provider_type: input.provider_type,
        base_url: parsed_url.to_string(),
        api_key,
        enabled: input.enabled,
        created_at: chrono::DateTime::default(),
        updated_at: chrono::DateTime::default(),
    };
    let db = state.db.clone();
    let inserted = crate::run_blocking(move || db.insert_provider(provider))
        .await
        .map_err(internal_error)?
        .map_err(|e| AitError::from_db_error(e).into_response())?;

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        request_id: request_id.0,
        username: session.username.clone(),
        action: "create".into(),
        resource: "provider".into(),
        resource_id: inserted.id.clone(),
        detail: None,
    });

    Ok((StatusCode::CREATED, Json(ProviderResponse::from(inserted))))
}

pub async fn list_providers(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionUser>,
) -> Result<Json<Vec<ProviderResponse>>, (StatusCode, Json<AitError>)> {
    let db = state.db.clone();
    let providers = crate::run_blocking(move || db.list_providers())
        .await
        .map_err(internal_error)?
        .map_err(|e| AitError::from_db_error(e).into_response())?;
    Ok(Json(
        providers.into_iter().map(ProviderResponse::from).collect(),
    ))
}

pub async fn get_provider(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionUser>,
    Path(id): Path<String>,
) -> Result<Json<ProviderResponse>, (StatusCode, Json<AitError>)> {
    let db = state.db.clone();
    let id_clone = id.clone();
    let provider = crate::run_blocking(move || db.get_provider(&id_clone))
        .await
        .map_err(internal_error)?
        .map_err(|e| AitError::from_db_error(e).into_response())?
        .ok_or_else(|| not_found(format!("Provider '{}' not found", id)))?;

    Ok(Json(ProviderResponse::from(provider)))
}

pub async fn get_provider_api_key(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Extension(request_id): Extension<RequestId>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AitError>)> {
    let db = state.db.clone();
    let id_clone = id.clone();
    let provider = crate::run_blocking(move || db.get_provider(&id_clone))
        .await
        .map_err(internal_error)?
        .map_err(|e| AitError::from_db_error(e).into_response())?
        .ok_or_else(|| not_found(format!("Provider '{}' not found", id)))?;

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        request_id: request_id.0,
        username: session.username,
        action: "view_api_key".into(),
        resource: "provider".into(),
        resource_id: provider.id.clone(),
        detail: None,
    });

    Ok(Json(serde_json::json!({
        "id": provider.id,
        "name": provider.name,
        "api_key": provider.api_key,
    })))
}

pub async fn update_provider(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Extension(request_id): Extension<RequestId>,
    Path(id): Path<String>,
    Json(input): Json<UpdateProviderRequest>,
) -> Result<Json<ProviderResponse>, (StatusCode, Json<AitError>)> {
    let name = input
        .name
        .map(|n| validate_string(&n, "name", 128, ident_chars))
        .transpose()?;
    let parsed_url = input
        .base_url
        .map(|u| -> Result<reqwest::Url, AitError> {
            let v = validate_string(&u, "base_url", 1024, |_| true)?;
            validate_base_url(&v)
        })
        .transpose()?;
    let api_key = match input.api_key {
        Some(k) => {
            let t = k.trim().to_string();
            if t.is_empty() {
                Some(String::new())
            } else {
                if t.len() > 512 {
                    return Err(
                        AitError::bad_request("api_key must not exceed 512 characters")
                            .into_response(),
                    );
                }
                Some(t)
            }
        }
        None => None,
    };
    if let Some(ref parsed) = parsed_url {
        ssrf::check_ssrf_config(
            parsed,
            &state.config.security.ssrf_allowed_cidrs,
            &state.ssrf_dns_cache,
            &id,
        )
        .await
        .map_err(|e| e.into_response())?;
    }
    let updates = ProviderUpdate {
        id: id.clone(),
        name,
        provider_type: input.provider_type,
        base_url: parsed_url.map(|u| u.to_string()),
        api_key,
        enabled: input.enabled,
    };
    let db = state.db.clone();
    let provider = crate::run_blocking(move || db.update_provider(&updates))
        .await
        .map_err(internal_error)?
        .map_err(|e| AitError::from_db_error(e).into_response())?;

    state.provider_cache.remove(&id);
    state
        .model_cache
        .retain(|_, v| v.0.as_ref().is_none_or(|(_, p)| p.id != id));
    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        request_id: request_id.0,
        username: session.username.clone(),
        action: "update".into(),
        resource: "provider".into(),
        resource_id: id,
        detail: None,
    });

    Ok(Json(ProviderResponse::from(provider)))
}

pub async fn delete_provider(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Extension(request_id): Extension<RequestId>,
    Path(id): Path<String>,
) -> Result<(StatusCode,), (StatusCode, Json<AitError>)> {
    let db = state.db.clone();
    let id_clone = id.clone();
    if !crate::run_blocking(move || db.delete_provider(&id_clone))
        .await
        .map_err(internal_error)?
        .map_err(|e| AitError::from_db_error(e).into_response())?
    {
        return Err(not_found(format!("Provider '{}' not found", id)));
    }

    state.provider_cache.remove(&id);
    state
        .model_cache
        .retain(|_, v| v.0.as_ref().is_none_or(|(_, p)| p.id != id));
    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        request_id: request_id.0,
        username: session.username.clone(),
        action: "delete".into(),
        resource: "provider".into(),
        resource_id: id,
        detail: None,
    });

    Ok((StatusCode::NO_CONTENT,))
}

static PROVIDER_TYPES: OnceLock<Vec<serde_json::Value>> = OnceLock::new();

pub async fn list_provider_types() -> Json<&'static Vec<serde_json::Value>> {
    Json(PROVIDER_TYPES.get_or_init(|| {
        ProviderType::iter()
            .map(|t| {
                serde_json::json!({
                    "type": t.as_ref(),
                    "display_name": t.get_message().expect("every ProviderType must carry a display message"),
                })
            })
            .collect()
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

    const BASE_URL: &str = "http://127.0.0.1:8080/";

    async fn setup() -> (Router, String) {
        let (state, _dir) = create_test_state();
        insert_test_user(&state.db, "alice", "secret123");
        let router = test_router(state);
        let cookie = login_and_cookie(&router, "alice", "secret123").await;
        (router, cookie)
    }

    fn create_body(name: &str) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "type": "openai_compat",
            "base_url": BASE_URL,
            "enabled": true,
        })
    }

    async fn create_provider(router: &Router, cookie: &str, name: &str) -> serde_json::Value {
        let resp = send_request(
            router,
            Method::POST,
            "/api/providers",
            Some(cookie),
            None,
            Some(create_body(name)),
        )
        .await;
        assert_eq!(resp.status, StatusCode::CREATED, "create should succeed");
        resp.json
    }

    #[tokio::test]
    async fn create_provider_returns_created() {
        let (router, cookie) = setup().await;
        let json = create_provider(&router, &cookie, "test-provider").await;
        assert_eq!(json["name"], "test-provider");
        assert_eq!(json["type"], "openai_compat");
        assert_eq!(json["base_url"], BASE_URL);
        assert_eq!(json["enabled"], serde_json::Value::Bool(true));
        assert!(!json["id"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn create_provider_empty_name_bad_request() {
        let (router, cookie) = setup().await;
        let resp = send_request(
            &router,
            Method::POST,
            "/api/providers",
            Some(&cookie),
            None,
            Some(create_body("   ")),
        )
        .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        assert_eq!(resp.json["code"], 400);
    }

    #[tokio::test]
    async fn create_provider_invalid_base_url_bad_request() {
        let (router, cookie) = setup().await;
        let mut body = create_body("test-provider");
        body["base_url"] = serde_json::json!("not-a-url");
        let resp = send_request(
            &router,
            Method::POST,
            "/api/providers",
            Some(&cookie),
            None,
            Some(body),
        )
        .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_providers_includes_created() {
        let (router, cookie) = setup().await;
        create_provider(&router, &cookie, "test-provider").await;
        let resp = send_request(
            &router,
            Method::GET,
            "/api/providers",
            Some(&cookie),
            None,
            None,
        )
        .await;
        assert_eq!(resp.status, StatusCode::OK);
        let providers = resp.json.as_array().expect("list should return an array");
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0]["name"], "test-provider");
        assert_eq!(providers[0]["base_url"], BASE_URL);
    }

    #[tokio::test]
    async fn update_provider_changes_fields() {
        let (router, cookie) = setup().await;
        let json = create_provider(&router, &cookie, "test-provider").await;
        let id = json["id"].as_str().unwrap();
        let resp = send_request(
            &router,
            Method::PUT,
            &format!("/api/providers/{id}"),
            Some(&cookie),
            None,
            Some(serde_json::json!({
                "base_url": "http://127.0.0.1:9090/",
                "enabled": false,
            })),
        )
        .await;
        assert_eq!(resp.status, StatusCode::OK);
        assert_eq!(resp.json["base_url"], "http://127.0.0.1:9090/");
        assert_eq!(resp.json["enabled"], serde_json::Value::Bool(false));
    }

    #[tokio::test]
    async fn delete_provider_removes_and_get_returns_404() {
        let (router, cookie) = setup().await;
        let json = create_provider(&router, &cookie, "test-provider").await;
        let id = json["id"].as_str().unwrap();
        let resp = send_request(
            &router,
            Method::DELETE,
            &format!("/api/providers/{id}"),
            Some(&cookie),
            None,
            None,
        )
        .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);
        let resp = send_request(
            &router,
            Method::GET,
            &format!("/api/providers/{id}"),
            Some(&cookie),
            None,
            None,
        )
        .await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn list_provider_types_non_empty() {
        let (router, cookie) = setup().await;
        let resp = send_request(
            &router,
            Method::GET,
            "/api/provider-types",
            Some(&cookie),
            None,
            None,
        )
        .await;
        assert_eq!(resp.status, StatusCode::OK);
        let types = resp.json.as_array().expect("types should be an array");
        assert!(!types.is_empty());
        assert!(types.iter().any(|t| t["type"] == "openai_compat"));
    }
}
