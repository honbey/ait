use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::app::AppState;
use crate::db::{AuditEvent, SessionUser};
use crate::error::{AitError, forbidden, internal_error, not_found};

#[derive(Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
    pub expires_at: Option<i64>,
}

#[derive(serde::Serialize)]
pub struct ApiKeyResponse {
    pub key: String,
    pub name: String,
    pub created_at: i64,
    pub expires_at: Option<i64>,
}

#[derive(serde::Serialize)]
pub struct ApiKeyListItem {
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
    Path(username): Path<String>,
    Json(input): Json<CreateApiKeyRequest>,
) -> Result<Json<ApiKeyResponse>, (StatusCode, Json<AitError>)> {
    if session.username != username {
        return Err(forbidden("You can only manage your own API keys"));
    }

    let expires_at: Option<DateTime<Utc>> = input
        .expires_at
        .map(|ts| {
            DateTime::from_timestamp(ts, 0)
                .ok_or_else(|| AitError::bad_request("Invalid expires_at").into_response())
        })
        .transpose()?;

    let db = state.db.clone();
    let username_clone = username.clone();
    let name = input.name.clone();
    let (stored, raw_key) =
        crate::run_blocking(move || db.insert_api_key(&username_clone, &name, expires_at))
            .await
            .map_err(internal_error)?;

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        username: session.username.clone(),
        action: "create".into(),
        resource: "api_key".into(),
        resource_id: stored.id.clone(),
        detail: None,
    });

    Ok(Json(ApiKeyResponse {
        key: raw_key,
        name: stored.name,
        created_at: stored.created_at.timestamp(),
        expires_at: stored.expires_at.map(|dt| dt.timestamp()),
    }))
}

pub async fn list_api_keys(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path(username): Path<String>,
) -> Result<Json<Vec<ApiKeyListItem>>, (StatusCode, Json<AitError>)> {
    if session.username != username {
        return Err(forbidden("You can only view your own API keys"));
    }

    // Fast single RocksDB get_cf (~10–50 µs); spawn_blocking overhead
    // (~5–20 µs) would exceed the work itself, so called directly.
    let user = state
        .db
        .get_user(&username)
        .map_err(internal_error)?
        .ok_or_else(|| not_found(format!("User '{}' not found", username)))?;

    let items: Vec<ApiKeyListItem> = user
        .api_keys
        .into_iter()
        .map(|k| ApiKeyListItem {
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
        .map_err(internal_error)?;
    state.api_key_cache.remove(&hash);

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        username: session.username.clone(),
        action: "delete".into(),
        resource: "api_key".into(),
        resource_id: key,
        detail: None,
    });

    Ok((StatusCode::NO_CONTENT,))
}

#[derive(Deserialize)]
pub struct ToggleApiKeyRequest {
    pub enabled: bool,
}

pub async fn toggle_api_key(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path((username, key_id)): Path<(String, String)>,
    Json(input): Json<ToggleApiKeyRequest>,
) -> Result<Json<ApiKeyListItem>, (StatusCode, Json<AitError>)> {
    if session.username != username {
        return Err(forbidden("You can only manage your own API keys"));
    }

    let db = state.db.clone();
    let username_clone = username.clone();
    let key_id_clone = key_id.clone();
    let (updated, hash) = crate::run_blocking(move || {
        db.toggle_api_key(&username_clone, &key_id_clone, input.enabled)
    })
    .await
    .map_err(internal_error)?;
    state.api_key_cache.remove(&hash);

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        username: session.username.clone(),
        action: "toggle".into(),
        resource: "api_key".into(),
        resource_id: key_id,
        detail: None,
    });

    Ok(Json(ApiKeyListItem {
        id: updated.id.clone(),
        key: updated.masked(),
        name: updated.name,
        created_at: updated.created_at.timestamp(),
        updated_at: updated.updated_at.timestamp(),
        enabled: updated.enabled,
        expires_at: updated.expires_at.map(|dt| dt.timestamp()),
    }))
}
