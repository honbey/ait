use std::sync::OnceLock;

use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::db::{AuditEvent, Model, Provider, ProviderType, SessionUser};
use crate::error::{AitError, internal_error, not_found};
use strum::{EnumMessage, IntoEnumIterator};

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
    created_at: i64,
    updated_at: i64,
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
            created_at: p.created_at.timestamp(),
            updated_at: p.updated_at.timestamp(),
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

impl From<UpdateProviderRequest> for Provider {
    fn from(r: UpdateProviderRequest) -> Self {
        Provider {
            id: String::new(),
            name: r.name.unwrap_or_default(),
            provider_type: r.provider_type.unwrap_or_default(),
            base_url: r.base_url.unwrap_or_default(),
            api_key: r.api_key,
            enabled: r.enabled.unwrap_or(true),
            created_at: chrono::DateTime::default(),
            updated_at: chrono::DateTime::default(),
        }
    }
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

// ── Handlers ──

pub async fn create_provider(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Json(input): Json<CreateProviderRequest>,
) -> Result<(StatusCode, Json<ProviderResponse>), (StatusCode, Json<AitError>)> {
    let provider: Provider = input.into();
    let db = state.db.clone();
    let inserted = crate::run_blocking(move || db.insert_provider(provider))
        .await
        .map_err(internal_error)?;

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
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
        .map_err(internal_error)?;
    Ok(Json(
        providers.into_iter().map(ProviderResponse::from).collect(),
    ))
}

pub async fn get_provider(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionUser>,
    Path(id): Path<String>,
) -> Result<Json<ProviderResponse>, (StatusCode, Json<AitError>)> {
    // Fast single RocksDB get_cf (~10–50 µs); spawn_blocking overhead
    // (~5–20 µs) would exceed the work itself, so called directly.
    let provider = state
        .db
        .get_provider(&id)
        .map_err(internal_error)?
        .ok_or_else(|| not_found(format!("Provider '{}' not found", id)))?;

    Ok(Json(ProviderResponse::from(provider)))
}

pub async fn get_provider_api_key(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionUser>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AitError>)> {
    // Fast single RocksDB get_cf (~10–50 µs); spawn_blocking overhead
    // (~5–20 µs) would exceed the work itself, so called directly.
    let provider = state
        .db
        .get_provider(&id)
        .map_err(internal_error)?
        .ok_or_else(|| not_found(format!("Provider '{}' not found", id)))?;

    Ok(Json(serde_json::json!({
        "id": provider.id,
        "name": provider.name,
        "api_key": provider.api_key,
    })))
}

pub async fn update_provider(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path(id): Path<String>,
    Json(input): Json<UpdateProviderRequest>,
) -> Result<Json<ProviderResponse>, (StatusCode, Json<AitError>)> {
    let mut updates: Provider = input.into();
    updates.id = id.clone();
    let db = state.db.clone();
    let provider = crate::run_blocking(move || db.update_provider(&updates))
        .await
        .map_err(internal_error)?;

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
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
    Path(id): Path<String>,
) -> Result<(StatusCode,), (StatusCode, Json<AitError>)> {
    let db = state.db.clone();
    let id_clone = id.clone();
    if !crate::run_blocking(move || db.delete_provider(&id_clone))
        .await
        .map_err(internal_error)?
    {
        return Err(not_found(format!("Provider '{}' not found", id)));
    }

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        username: session.username.clone(),
        action: "delete".into(),
        resource: "provider".into(),
        resource_id: id,
        detail: None,
    });

    Ok((StatusCode::NO_CONTENT,))
}

static PROVIDER_TYPES: OnceLock<Vec<serde_json::Value>> = OnceLock::new();

pub async fn list_provider_types() -> Json<Vec<serde_json::Value>> {
    Json(PROVIDER_TYPES.get_or_init(|| {
        ProviderType::iter()
            .map(|t| {
                serde_json::json!({
                    "type": t.as_ref(),
                    "display_name": t.get_message().expect("every ProviderType must carry a display message"),
                })
            })
            .collect()
    }).clone())
}

// ── Model types ──

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ModelResponse {
    id: String,
    name: String,
    provider_id: String,
    upstream_model: String,
    enabled: bool,
    created_at: i64,
    updated_at: i64,
}

impl From<Model> for ModelResponse {
    fn from(m: Model) -> Self {
        Self {
            id: m.id,
            name: m.name,
            provider_id: m.provider_id,
            upstream_model: m.upstream_model,
            enabled: m.enabled,
            created_at: m.created_at.timestamp(),
            updated_at: m.updated_at.timestamp(),
        }
    }
}

#[derive(Deserialize)]
pub struct CreateModelRequest {
    pub name: String,
    pub provider_id: String,
    pub upstream_model: String,
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct UpdateModelRequest {
    pub provider_id: Option<String>,
    pub upstream_model: Option<String>,
    pub enabled: Option<bool>,
}

impl From<UpdateModelRequest> for Model {
    fn from(r: UpdateModelRequest) -> Self {
        Model {
            id: String::new(),
            name: String::new(),
            provider_id: r.provider_id.unwrap_or_default(),
            upstream_model: r.upstream_model.unwrap_or_default(),
            enabled: r.enabled.unwrap_or(true),
            created_at: chrono::DateTime::default(),
            updated_at: chrono::DateTime::default(),
        }
    }
}

impl From<CreateModelRequest> for Model {
    fn from(r: CreateModelRequest) -> Self {
        Model {
            id: String::new(),
            name: r.name,
            provider_id: r.provider_id,
            upstream_model: r.upstream_model,
            enabled: r.enabled,
            created_at: chrono::DateTime::default(),
            updated_at: chrono::DateTime::default(),
        }
    }
}

// ── Model handlers ──

pub async fn create_model(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Json(input): Json<CreateModelRequest>,
) -> Result<(StatusCode, Json<ModelResponse>), (StatusCode, Json<AitError>)> {
    let model: Model = input.into();
    let db = state.db.clone();
    let inserted = crate::run_blocking(move || db.insert_model(model))
        .await
        .map_err(internal_error)?;

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        username: session.username.clone(),
        action: "create".into(),
        resource: "model".into(),
        resource_id: inserted.name.clone(),
        detail: None,
    });

    Ok((StatusCode::CREATED, Json(ModelResponse::from(inserted))))
}

pub async fn list_models(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionUser>,
) -> Result<Json<Vec<ModelResponse>>, (StatusCode, Json<AitError>)> {
    let db = state.db.clone();
    let models = crate::run_blocking(move || db.list_models())
        .await
        .map_err(internal_error)?;
    Ok(Json(models.into_iter().map(ModelResponse::from).collect()))
}

pub async fn delete_model(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path(name): Path<String>,
) -> Result<(StatusCode,), (StatusCode, Json<AitError>)> {
    let db = state.db.clone();
    let name_clone = name.clone();
    crate::run_blocking(move || db.delete_model(&name_clone))
        .await
        .map_err(internal_error)?;

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        username: session.username.clone(),
        action: "delete".into(),
        resource: "model".into(),
        resource_id: name,
        detail: None,
    });

    Ok((StatusCode::NO_CONTENT,))
}

pub async fn update_model(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path(name): Path<String>,
    Json(input): Json<UpdateModelRequest>,
) -> Result<Json<ModelResponse>, (StatusCode, Json<AitError>)> {
    let mut updates: Model = input.into();
    updates.name = name.clone();
    let db = state.db.clone();
    let model = crate::run_blocking(move || db.update_model(&updates))
        .await
        .map_err(internal_error)?;

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        username: session.username.clone(),
        action: "update".into(),
        resource: "model".into(),
        resource_id: name,
        detail: None,
    });

    Ok(Json(ModelResponse::from(model)))
}
