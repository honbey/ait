use crate::app::AppState;
use crate::db::{Permission, UserRole};
use crate::error::AitError;
use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::Response,
};

#[derive(Clone)]
pub struct SessionUser {
    pub username: String,
    pub role: UserRole,
    pub allowed: Vec<Permission>,
}

/// Extract `session_key` from Authorization header (Bearer) or Cookie header.
fn extract_session_key(headers: &HeaderMap) -> Option<&str> {
    if let Some(key) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        return Some(key);
    }
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
    mut req: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    // Admin endpoints always require authentication
    let session_key = extract_session_key(req.headers())
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(AitError::unauthorized())))?;

    let session = state
        .db
        .get_session(session_key)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(AitError::internal_error("Database error"))))?
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(AitError::unauthorized())))?;

    if session.expires_at <= chrono::Utc::now() {
        return Err((StatusCode::UNAUTHORIZED, Json(AitError::unauthorized())));
    }

    let user = state
        .db
        .get_user(&session.username)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(AitError::internal_error("Database error"))))?
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(AitError::unauthorized())))?;

    let session_user = SessionUser {
        username: user.username,
        role: user.role,
        allowed: user.allowed,
    };

    req.extensions_mut().insert(session_user);
    Ok(next.run(req).await)
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
