use crate::app::AppState;
use crate::providers::OpenAIError;
use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};

pub async fn auth_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<OpenAIError>)> {
    if !state.config.auth.enabled {
        return Ok(next.run(req).await);
    }

    let expected_token = state.config.auth.token.as_deref().unwrap_or("");
    check_bearer_token(req.headers(), expected_token)?;

    Ok(next.run(req).await)
}

pub async fn admin_auth_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<OpenAIError>)> {
    // Admin endpoints always require authentication regardless of auth.enabled
    let expected_token = state.config.auth.admin_token.as_deref().unwrap_or("");

    check_bearer_token(req.headers(), expected_token)?;

    Ok(next.run(req).await)
}

fn check_bearer_token(
    headers: &HeaderMap,
    expected_token: &str,
) -> Result<(), (StatusCode, Json<OpenAIError>)> {
    let auth_header = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    if !auth_header.starts_with("Bearer ") || &auth_header[7..] != expected_token {
        return Err((StatusCode::UNAUTHORIZED, Json(OpenAIError::unauthorized())));
    }

    Ok(())
}
