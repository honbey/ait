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

fn full_access(username: &str) -> SessionUser {
    SessionUser {
        username: username.to_string(),
        role: UserRole::Admin,
        allowed: vec![],
    }
}

/// Extract Bearer token from Authorization header.
fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

/// Extract `session_key` from Authorization header (Bearer) or Cookie header.
pub fn extract_session_key(headers: &HeaderMap) -> Option<&str> {
    if let Some(key) = extract_bearer_token(headers) {
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
    mut req: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    if !state.config.auth.enabled {
        req.extensions_mut().insert(full_access("anonymous"));
        return Ok(next.run(req).await);
    }

    let token = extract_bearer_token(req.headers())
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(AitError::unauthorized())))?;

    // Check if token is a session key
    if let Ok(Some(session)) = state.db.get_session(token) {
        if session.expires_at <= chrono::Utc::now() {
            return Err((StatusCode::UNAUTHORIZED, Json(AitError::unauthorized())));
        }
        let user = state
            .db
            .get_user(&session.username)
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(AitError::internal_error("Database error")),
                )
            })?
            .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(AitError::unauthorized())))?;

        req.extensions_mut().insert(SessionUser {
            username: user.username,
            role: user.role,
            allowed: user.allowed,
        });
        return Ok(next.run(req).await);
    }

    // Check if token is an API key
    let key_info = state
        .db
        .get_user_by_api_key(token)
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AitError::internal_error("Database error")),
            )
        })?
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(AitError::unauthorized())))?;

    let user = state
        .db
        .get_user(&key_info.username)
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AitError::internal_error("Database error")),
            )
        })?
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(AitError::unauthorized())))?;

    // Check that the specific API key is enabled and not expired
    let key = user
        .api_keys
        .iter()
        .find(|k| k.id == key_info.id && k.enabled)
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(AitError::unauthorized())))?;

    if let Some(expires_at) = &key.expires_at {
        if *expires_at <= chrono::Utc::now() {
            return Err((StatusCode::UNAUTHORIZED, Json(AitError::unauthorized())));
        }
    }

    // API key users are always User role (never Admin)
    req.extensions_mut().insert(SessionUser {
        username: user.username,
        role: UserRole::User,
        allowed: user.allowed,
    });
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
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AitError::internal_error("Database error")),
            )
        })?
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(AitError::unauthorized())))?;

    if session.expires_at <= chrono::Utc::now() {
        return Err((StatusCode::UNAUTHORIZED, Json(AitError::unauthorized())));
    }

    let user = state
        .db
        .get_user(&session.username)
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AitError::internal_error("Database error")),
            )
        })?
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(AitError::unauthorized())))?;

    let session_user = SessionUser {
        username: user.username,
        role: user.role,
        allowed: user.allowed,
    };

    req.extensions_mut().insert(session_user);
    Ok(next.run(req).await)
}
