use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::app::AppState;
use crate::error::AitError;
use crate::middleware::SessionUser;

#[derive(Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
    pub expires_at: Option<String>,
}

#[derive(serde::Serialize)]
pub struct ApiKeyResponse {
    pub key: String,
    pub name: String,
    pub created_at: String,
    pub expires_at: Option<String>,
}

#[derive(serde::Serialize)]
pub struct ApiKeyListItem {
    pub id: String,
    pub key: String,
    pub name: String,
    pub created_at: String,
    pub enabled: bool,
    pub expires_at: Option<String>,
}

pub async fn create_api_key_handler(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path(username): Path<String>,
    Json(input): Json<CreateApiKeyRequest>,
) -> Result<Json<ApiKeyResponse>, (StatusCode, Json<AitError>)> {
    if session.role != crate::db::UserRole::Admin && session.username != username {
        return Err(forbidden());
    }

    let expires_at: Option<DateTime<Utc>> = input
        .expires_at
        .as_deref()
        .map(|s| {
            DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(AitError::bad_request(format!("Invalid expires_at: {}", e))),
                    )
                })
        })
        .transpose()?;

    let (stored, raw_key) = state
        .db
        .insert_api_key(&username, &input.name, expires_at)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(AitError::bad_request(e))))?;

    Ok(Json(ApiKeyResponse {
        key: raw_key,
        name: stored.name,
        created_at: stored.created_at.to_rfc3339(),
        expires_at: stored.expires_at.map(|dt| dt.to_rfc3339()),
    }))
}

pub async fn list_api_keys_handler(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path(username): Path<String>,
) -> Result<Json<Vec<ApiKeyListItem>>, (StatusCode, Json<AitError>)> {
    if session.role != crate::db::UserRole::Admin && session.username != username {
        return Err(forbidden());
    }

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
            created_at: k.created_at.to_rfc3339(),
            enabled: k.enabled,
            expires_at: k.expires_at.map(|dt| dt.to_rfc3339()),
        })
        .collect();
    Ok(Json(items))
}

pub async fn delete_api_key_handler(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path((username, key)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AitError>)> {
    if session.role != crate::db::UserRole::Admin && session.username != username {
        return Err(forbidden());
    }

    state
        .db
        .delete_api_key(&username, &key)
        .map_err(internal_error)?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub struct ToggleApiKeyRequest {
    pub enabled: bool,
}

pub async fn toggle_api_key_handler(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path((username, key_id)): Path<(String, String)>,
    Json(input): Json<ToggleApiKeyRequest>,
) -> Result<Json<ApiKeyListItem>, (StatusCode, Json<AitError>)> {
    if session.role != crate::db::UserRole::Admin && session.username != username {
        return Err(forbidden());
    }
    let updated = state
        .db
        .toggle_api_key(&username, &key_id, input.enabled)
        .map_err(internal_error)?;
    Ok(Json(ApiKeyListItem {
        id: updated.id.clone(),
        key: updated.masked(),
        name: updated.name,
        created_at: updated.created_at.to_rfc3339(),
        enabled: updated.enabled,
        expires_at: updated.expires_at.map(|dt| dt.to_rfc3339()),
    }))
}

// --- Helpers ---

fn internal_error(e: impl std::fmt::Display) -> (StatusCode, Json<AitError>) {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(AitError::internal_error(e.to_string())))
}

fn not_found(msg: impl Into<String>) -> (StatusCode, Json<AitError>) {
    (StatusCode::NOT_FOUND, Json(AitError::not_found(msg)))
}

fn forbidden() -> (StatusCode, Json<AitError>) {
    (StatusCode::FORBIDDEN, Json(AitError {
        message: "Admin privileges required".to_string(),
        code: 403,
        r#type: "forbidden".to_string(),
    }))
}
