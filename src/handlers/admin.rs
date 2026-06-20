use axum::{
    Extension, Json as AxumJson,
    extract::{Json, Path, State},
    http::StatusCode,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::db::{Model, Provider, ProviderType, UserRole};
use crate::error::{AitError, forbidden, internal_error, not_found};
use crate::middleware::SessionUser;

// --- Provider request/response types ---

#[derive(Serialize)]
pub struct ProviderResponse {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub provider_type: ProviderType,
    pub base_url: String,
    pub api_key: Option<String>,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<Provider> for ProviderResponse {
    fn from(p: Provider) -> Self {
        ProviderResponse {
            api_key: p.masked_api_key(),
            id: p.id,
            name: p.name,
            provider_type: p.provider_type,
            base_url: p.base_url,
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
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct UpdateProviderRequest {
    pub name: String,
    #[serde(rename = "type")]
    pub provider_type: ProviderType,
    pub base_url: String,
    pub api_key: Option<String>,
    pub enabled: bool,
}

// --- Model request/response types ---

#[derive(Serialize)]
pub struct ModelResponse {
    pub id: String,
    pub name: String,
    pub provider_id: String,
    pub upstream_model: String,
    pub enabled: bool,
    pub created_at: i64,
}

impl From<Model> for ModelResponse {
    fn from(m: Model) -> Self {
        ModelResponse {
            id: m.id,
            name: m.name,
            provider_id: m.provider_id,
            upstream_model: m.upstream_model,
            enabled: m.enabled,
            created_at: m.created_at.timestamp(),
        }
    }
}

#[derive(Deserialize)]
pub struct CreateModelRequest {
    pub name: String,
    pub provider_id: String,
    pub upstream_model: String,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct UpdateModelRequest {
    pub provider_id: String,
    pub upstream_model: String,
    pub enabled: bool,
}

// --- Provider CRUD ---

pub async fn create_provider(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Json(input): Json<CreateProviderRequest>,
) -> Result<(StatusCode, Json<ProviderResponse>), (StatusCode, Json<AitError>)> {
    if session.role != UserRole::Admin {
        return Err(forbidden());
    }
    let provider = Provider {
        id: String::new(),
        name: input.name,
        provider_type: input.provider_type,
        base_url: input.base_url,
        api_key: input.api_key,
        enabled: input.enabled,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let inserted = state.db.insert_provider(provider).map_err(internal_error)?;

    Ok((StatusCode::CREATED, Json(ProviderResponse::from(inserted))))
}

pub async fn list_providers(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
) -> Result<Json<Vec<ProviderResponse>>, (StatusCode, Json<AitError>)> {
    let providers = state.db.list_providers().map_err(internal_error)?;
    let masked: Vec<ProviderResponse> = match session.role {
        UserRole::Admin => providers.into_iter().map(ProviderResponse::from).collect(),
        UserRole::User => providers
            .into_iter()
            .filter(|p| session.allowed.iter().any(|a| a.provider_id == p.id))
            .map(ProviderResponse::from)
            .collect(),
    };
    Ok(Json(masked))
}

pub async fn get_provider(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path(id): Path<String>,
) -> Result<Json<ProviderResponse>, (StatusCode, Json<AitError>)> {
    let provider = state
        .db
        .get_provider(&id)
        .map_err(internal_error)?
        .ok_or_else(|| not_found(format!("Provider '{}' not found", id)))?;

    if session.role != UserRole::Admin
        && !session.allowed.iter().any(|a| a.provider_id == provider.id)
    {
        return Err(forbidden());
    }

    Ok(Json(ProviderResponse::from(provider)))
}

pub async fn get_provider_api_key(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path(id): Path<String>,
) -> Result<AxumJson<serde_json::Value>, (StatusCode, Json<AitError>)> {
    if session.role != UserRole::Admin {
        return Err(forbidden());
    }
    let provider = state
        .db
        .get_provider(&id)
        .map_err(internal_error)?
        .ok_or_else(|| not_found(format!("Provider '{}' not found", id)))?;

    Ok(AxumJson(serde_json::json!({
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
    if session.role != UserRole::Admin {
        return Err(forbidden());
    }
    let updates = Provider {
        id: id.clone(),
        name: input.name,
        provider_type: input.provider_type,
        base_url: input.base_url,
        api_key: input.api_key,
        enabled: input.enabled,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let provider = state
        .db
        .update_provider(&id, &updates)
        .map_err(internal_error)?
        .ok_or_else(|| not_found(format!("Provider '{}' not found", id)))?;

    Ok(Json(ProviderResponse::from(provider)))
}

pub async fn delete_provider(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path(id): Path<String>,
) -> Result<(StatusCode,), (StatusCode, Json<AitError>)> {
    if session.role != UserRole::Admin {
        return Err(forbidden());
    }
    state.db.delete_provider(&id).map_err(internal_error)?;
    Ok((StatusCode::NO_CONTENT,))
}

// --- Model CRUD ---

pub async fn create_model(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Json(input): Json<CreateModelRequest>,
) -> Result<(StatusCode, Json<ModelResponse>), (StatusCode, Json<AitError>)> {
    if session.role != UserRole::Admin {
        return Err(forbidden());
    }
    let model = Model {
        id: String::new(),
        name: input.name,
        provider_id: input.provider_id,
        upstream_model: input.upstream_model,
        enabled: input.enabled,
        created_at: Utc::now(),
    };
    let inserted = state
        .db
        .insert_model(model)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(AitError::bad_request(e))))?;

    Ok((StatusCode::CREATED, Json(ModelResponse::from(inserted))))
}

pub async fn list_models(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
) -> Result<Json<Vec<ModelResponse>>, (StatusCode, Json<AitError>)> {
    let models = state.db.list_models().map_err(internal_error)?;
    let filtered: Vec<ModelResponse> = match session.role {
        UserRole::Admin => models.into_iter().map(ModelResponse::from).collect(),
        UserRole::User => models
            .into_iter()
            .filter(|m| {
                session.allowed.iter().any(|a| {
                    a.provider_id == m.provider_id
                        && (a.model_names.is_empty() || a.model_names.contains(&m.name))
                })
            })
            .map(ModelResponse::from)
            .collect(),
    };
    Ok(Json(filtered))
}

pub async fn delete_model(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path(name): Path<String>,
) -> Result<(StatusCode,), (StatusCode, Json<AitError>)> {
    if session.role != UserRole::Admin {
        return Err(forbidden());
    }
    state.db.delete_model(&name).map_err(internal_error)?;
    Ok((StatusCode::NO_CONTENT,))
}

pub async fn update_model(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path(name): Path<String>,
    Json(input): Json<UpdateModelRequest>,
) -> Result<Json<ModelResponse>, (StatusCode, Json<AitError>)> {
    if session.role != UserRole::Admin {
        return Err(forbidden());
    }
    let updates = Model {
        id: String::new(),
        name: name.clone(),
        provider_id: input.provider_id,
        upstream_model: input.upstream_model,
        enabled: input.enabled,
        created_at: Utc::now(),
    };
    let model = state
        .db
        .update_model(&name, &updates)
        .map_err(internal_error)?;
    Ok(Json(ModelResponse::from(model)))
}
