use axum::{
    Extension,
    extract::{Path, State, Json},
    http::StatusCode,
    Json as AxumJson,
};
use crate::app::AppState;
use crate::db::{Model, Provider, UserRole};
use crate::error::AitError;
use crate::middleware::SessionUser;

// --- Provider CRUD ---

pub async fn create_provider(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Json(input): Json<Provider>,
) -> Result<(StatusCode, Json<Provider>), (StatusCode, Json<AitError>)> {
    if session.role != UserRole::Admin {
        return Err(forbidden());
    }
    let inserted = state
        .db
        .insert_provider(input)
        .map_err(internal_error)?;

    let masked = mask_provider_api_key(inserted);
    Ok((StatusCode::CREATED, Json(masked)))
}

pub async fn list_providers(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
) -> Result<Json<Vec<Provider>>, (StatusCode, Json<AitError>)> {
    let providers = state.db.list_providers().map_err(internal_error)?;
    let masked: Vec<Provider> = providers
        .into_iter()
        .filter(|p| match session.role {
            UserRole::Admin => true,
            UserRole::User => session.allowed.iter().any(|a| a.provider_id == p.id),
        })
        .map(mask_provider_api_key)
        .collect();
    Ok(Json(masked))
}

pub async fn get_provider(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path(id): Path<String>,
) -> Result<Json<Provider>, (StatusCode, Json<AitError>)> {
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

    let masked = mask_provider_api_key(provider);
    Ok(Json(masked))
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
    Json(updates): Json<Provider>,
) -> Result<Json<Provider>, (StatusCode, Json<AitError>)> {
    if session.role != UserRole::Admin {
        return Err(forbidden());
    }
    let provider = state
        .db
        .update_provider(&id, &updates)
        .map_err(internal_error)?
        .ok_or_else(|| not_found(format!("Provider '{}' not found", id)))?;

    let masked = mask_provider_api_key(provider);
    Ok(Json(masked))
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
    Json(input): Json<Model>,
) -> Result<(StatusCode, Json<Model>), (StatusCode, Json<AitError>)> {
    if session.role != UserRole::Admin {
        return Err(forbidden());
    }
    let inserted = state
        .db
        .insert_model(input)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(AitError::bad_request(e))))?;

    Ok((StatusCode::CREATED, Json(inserted)))
}

pub async fn list_models(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
) -> Result<Json<Vec<Model>>, (StatusCode, Json<AitError>)> {
    let models = state.db.list_models().map_err(internal_error)?;
    let filtered: Vec<Model> = match session.role {
        UserRole::Admin => models,
        UserRole::User => models
            .into_iter()
            .filter(|m| {
                session.allowed.iter().any(|a| {
                    a.provider_id == m.provider_id
                        && (a.model_names.is_empty()
                            || a.model_names.contains(&m.name))
                })
            })
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
    Json(updates): Json<Model>,
) -> Result<Json<Model>, (StatusCode, Json<AitError>)> {
    if session.role != UserRole::Admin {
        return Err(forbidden());
    }
    let model = state
        .db
        .update_model(&name, &updates)
        .map_err(internal_error)?;
    Ok(Json(model))
}

// --- Helpers ---

fn mask_provider_api_key(mut provider: Provider) -> Provider {
    provider.api_key = provider.masked_api_key();
    provider
}

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
