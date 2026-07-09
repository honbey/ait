use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::db::{AuditEvent, Model, SessionUser};
use crate::error::{AitError, internal_error, not_found};

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
        .map_err(internal_error)?
        .map_err(|e| AitError::from_db_error(e).into_response())?;

    state.model_cache.clear();
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
        .map_err(internal_error)?
        .map_err(internal_error)?;
    Ok(Json(models.into_iter().map(ModelResponse::from).collect()))
}

pub async fn get_model(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionUser>,
    Path(name): Path<String>,
) -> Result<Json<ModelResponse>, (StatusCode, Json<AitError>)> {
    let db = state.db.clone();
    let name_clone = name.clone();
    let model = crate::run_blocking(move || db.get_model(&name_clone))
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?
        .ok_or_else(|| not_found(format!("Model '{}' not found", name)))?;

    Ok(Json(ModelResponse::from(model)))
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
        .map_err(internal_error)?
        .map_err(internal_error)?;

    state.model_cache.clear();
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
        .map_err(internal_error)?
        .map_err(internal_error)?;

    state.model_cache.clear();
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
