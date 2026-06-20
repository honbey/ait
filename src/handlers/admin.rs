use axum::{
    Extension, Json as AxumJson,
    extract::{Json, Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::db::{Model, Provider, ProviderType, SessionUser, UserRole};
use crate::error::{AitError, forbidden, internal_error, not_found, require_admin};

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
    pub updated_at: i64,
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
            updated_at: m.updated_at.timestamp(),
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

// --- From request types ---

impl From<CreateProviderRequest> for Provider {
    fn from(input: CreateProviderRequest) -> Self {
        Provider {
            id: Default::default(),
            name: input.name,
            provider_type: input.provider_type,
            base_url: input.base_url,
            api_key: input.api_key,
            enabled: input.enabled,
            created_at: Default::default(),
            updated_at: Default::default(),
        }
    }
}

impl From<UpdateProviderRequest> for Provider {
    fn from(input: UpdateProviderRequest) -> Self {
        Provider {
            id: Default::default(),
            name: input.name,
            provider_type: input.provider_type,
            base_url: input.base_url,
            api_key: input.api_key,
            enabled: input.enabled,
            created_at: Default::default(),
            updated_at: Default::default(),
        }
    }
}

impl From<CreateModelRequest> for Model {
    fn from(input: CreateModelRequest) -> Self {
        Model {
            id: Default::default(),
            name: input.name,
            provider_id: input.provider_id,
            upstream_model: input.upstream_model,
            enabled: input.enabled,
            created_at: Default::default(),
            updated_at: Default::default(),
        }
    }
}

impl From<UpdateModelRequest> for Model {
    fn from(input: UpdateModelRequest) -> Self {
        Model {
            id: Default::default(),
            name: Default::default(),
            provider_id: input.provider_id,
            upstream_model: input.upstream_model,
            enabled: input.enabled,
            created_at: Default::default(),
            updated_at: Default::default(),
        }
    }
}

// --- Provider CRUD ---

pub async fn create_provider(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Json(input): Json<CreateProviderRequest>,
) -> Result<(StatusCode, Json<ProviderResponse>), (StatusCode, Json<AitError>)> {
    require_admin(&session)?;
    let inserted = state.db.insert_provider(input.into())?;

    Ok((StatusCode::CREATED, Json(ProviderResponse::from(inserted))))
}

pub async fn list_providers(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
) -> Result<Json<Vec<ProviderResponse>>, (StatusCode, Json<AitError>)> {
    let providers = state.db.list_providers()?;
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
        return Err(forbidden("Admin privileges required"));
    }

    Ok(Json(ProviderResponse::from(provider)))
}

pub async fn get_provider_api_key(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path(id): Path<String>,
) -> Result<AxumJson<serde_json::Value>, (StatusCode, Json<AitError>)> {
    require_admin(&session)?;
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
    require_admin(&session)?;
    let mut updates: Provider = input.into();
    updates.id = id;
    let provider = state.db.update_provider(&updates)?;

    Ok(Json(ProviderResponse::from(provider)))
}

pub async fn delete_provider(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path(id): Path<String>,
) -> Result<(StatusCode,), (StatusCode, Json<AitError>)> {
    require_admin(&session)?;
    state.db.delete_provider(&id)?;
    Ok((StatusCode::NO_CONTENT,))
}

// --- Model CRUD ---

pub async fn create_model(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Json(input): Json<CreateModelRequest>,
) -> Result<(StatusCode, Json<ModelResponse>), (StatusCode, Json<AitError>)> {
    require_admin(&session)?;
    let inserted = state.db.insert_model(input.into())?;

    Ok((StatusCode::CREATED, Json(ModelResponse::from(inserted))))
}

pub async fn list_models(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
) -> Result<Json<Vec<ModelResponse>>, (StatusCode, Json<AitError>)> {
    let models = state.db.list_models()?;
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
    require_admin(&session)?;
    state.db.delete_model(&name)?;
    Ok((StatusCode::NO_CONTENT,))
}

pub async fn update_model(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path(name): Path<String>,
    Json(input): Json<UpdateModelRequest>,
) -> Result<Json<ModelResponse>, (StatusCode, Json<AitError>)> {
    require_admin(&session)?;
    let mut updates: Model = input.into();
    updates.name = name;
    let model = state.db.update_model(&updates)?;
    Ok(Json(ModelResponse::from(model)))
}
