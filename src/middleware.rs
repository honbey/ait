use crate::app::AppState;
use crate::error::AitError;
use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::Response,
};

fn extract_session_key(headers: &HeaderMap) -> Option<&str> {
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    for pair in cookie.split(';') {
        let pair = pair.trim();
        if let Some(value) = pair.strip_prefix("session_key=") {
            return Some(value);
        }
    }
    None
}

pub async fn auth_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<AitError>)> {
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
) -> Result<Response, (StatusCode, Json<AitError>)> {
    // Admin endpoints always require authentication
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    // Check session key from Authorization header (Bearer <session_key>)
    if auth_header.starts_with("Bearer ")
        && state
            .db
            .is_valid_session(&auth_header[7..])
            .unwrap_or(false)
    {
        return Ok(next.run(req).await);
    }

    // Check session key from Cookie header (for web login)
    if let Some(session_key) = extract_session_key(req.headers())
        && state.db.is_valid_session(session_key).unwrap_or(false)
    {
        return Ok(next.run(req).await);
    }

    Err((StatusCode::UNAUTHORIZED, Json(AitError::unauthorized())))
}

fn check_bearer_token(
    headers: &HeaderMap,
    expected_token: &str,
) -> Result<(), (StatusCode, Json<AitError>)> {
    let auth_header = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    if !auth_header.starts_with("Bearer ") || &auth_header[7..] != expected_token {
        return Err((StatusCode::UNAUTHORIZED, Json(AitError::unauthorized())));
    }

    Ok(())
}
