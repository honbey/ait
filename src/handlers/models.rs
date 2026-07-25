use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::serde::ts_seconds;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::db::{AuditEvent, Model, RequestId, SessionUser};
use crate::error::{AitError, internal_error, not_found};
use crate::handlers::{model_name_chars, upstream_model_chars, uuid_chars, validate_string};

// ── Model types ──

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ModelResponse {
    id: String,
    name: String,
    provider_id: String,
    upstream_model: String,
    enabled: bool,
    #[serde(with = "ts_seconds")]
    created_at: DateTime<Utc>,
    #[serde(with = "ts_seconds")]
    updated_at: DateTime<Utc>,
}

impl From<Model> for ModelResponse {
    fn from(m: Model) -> Self {
        Self {
            id: m.id,
            name: m.name,
            provider_id: m.provider_id,
            upstream_model: m.upstream_model,
            enabled: m.enabled,
            created_at: m.created_at,
            updated_at: m.updated_at,
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
    Extension(request_id): Extension<RequestId>,
    Json(input): Json<CreateModelRequest>,
) -> Result<(StatusCode, Json<ModelResponse>), (StatusCode, Json<AitError>)> {
    let name = validate_string(&input.name, "name", 128, model_name_chars)?;
    let provider_id = validate_string(&input.provider_id, "provider_id", 40, uuid_chars)?;
    let upstream_model = validate_string(
        &input.upstream_model,
        "upstream_model",
        128,
        upstream_model_chars,
    )?;
    let model = Model {
        id: String::new(),
        name,
        provider_id,
        upstream_model,
        enabled: input.enabled,
        created_at: chrono::DateTime::default(),
        updated_at: chrono::DateTime::default(),
    };
    let db = state.db.clone();
    let inserted = crate::run_blocking(move || db.insert_model(model))
        .await
        .map_err(internal_error)?
        .map_err(|e| AitError::from_db_error(e).into_response())?;

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        request_id: request_id.0,
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
        .map_err(|e| AitError::from_db_error(e).into_response())?;
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
        .map_err(|e| AitError::from_db_error(e).into_response())?
        .ok_or_else(|| not_found(format!("Model '{}' not found", name)))?;

    Ok(Json(ModelResponse::from(model)))
}

pub async fn delete_model(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Extension(request_id): Extension<RequestId>,
    Path(name): Path<String>,
) -> Result<(StatusCode,), (StatusCode, Json<AitError>)> {
    let db = state.db.clone();
    let name_clone = name.clone();
    if !crate::run_blocking(move || db.delete_model(&name_clone))
        .await
        .map_err(internal_error)?
        .map_err(|e| AitError::from_db_error(e).into_response())?
    {
        return Err(not_found(format!("Model '{}' not found", name)));
    }

    state.model_cache.remove(&name);
    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        request_id: request_id.0,
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
    Extension(request_id): Extension<RequestId>,
    Path(name): Path<String>,
    Json(input): Json<UpdateModelRequest>,
) -> Result<Json<ModelResponse>, (StatusCode, Json<AitError>)> {
    let model_name = validate_string(&name, "name", 128, model_name_chars)?;
    let provider_id = input
        .provider_id
        .map(|v| validate_string(&v, "provider_id", 40, uuid_chars))
        .transpose()?;
    let upstream_model = input
        .upstream_model
        .map(|v| validate_string(&v, "upstream_model", 128, upstream_model_chars))
        .transpose()?;
    let updates = Model {
        id: String::new(),
        name: model_name.clone(),
        provider_id: provider_id.unwrap_or_default(),
        upstream_model: upstream_model.unwrap_or_default(),
        enabled: input.enabled.unwrap_or(true),
        created_at: chrono::DateTime::default(),
        updated_at: chrono::DateTime::default(),
    };
    let db = state.db.clone();
    let model = crate::run_blocking(move || db.update_model(&updates))
        .await
        .map_err(internal_error)?
        .map_err(|e| AitError::from_db_error(e).into_response())?;

    state.model_cache.remove(&model_name);
    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        request_id: request_id.0,
        username: session.username.clone(),
        action: "update".into(),
        resource: "model".into(),
        resource_id: model_name,
        detail: None,
    });

    Ok(Json(ModelResponse::from(model)))
}
