use crate::app::AppState;
use crate::db::{AccessEvent, SessionUser, UserRole};
use crate::error::{AitError, db_error, too_many_requests};
use axum::{
    Json,
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::Response,
};
use chrono::Utc;
use std::net::{IpAddr, SocketAddr};
use std::time::Instant;

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
        .ok_or_else(|| AitError::unauthorized().into_response())?;

    // Check if token is an API key
    let key_info = state
        .db
        .get_user_by_api_key(token)
        .map_err(|_| db_error())?
        .ok_or_else(|| AitError::unauthorized().into_response())?;

    let user = state
        .db
        .get_user(&key_info.username)
        .map_err(|_| db_error())?
        .ok_or_else(|| AitError::unauthorized().into_response())?;

    // Check that the specific API key is enabled and not expired
    let key = user
        .api_keys
        .iter()
        .find(|k| k.id == key_info.id && k.enabled)
        .ok_or_else(|| AitError::unauthorized().into_response())?;

    if key.expires_at.is_some_and(|exp| exp <= chrono::Utc::now()) {
        return Err(AitError::unauthorized().into_response());
    }

    req.extensions_mut().insert(SessionUser {
        username: user.username,
        role: user.role,
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
        .ok_or_else(|| AitError::unauthorized().into_response())?;

    let session = state
        .db
        .get_session(session_key)
        .map_err(|_| db_error())?
        .ok_or_else(|| AitError::unauthorized().into_response())?;

    if session.is_expired() {
        return Err(AitError::unauthorized().into_response());
    }

    let user = state
        .db
        .get_user(&session.username)
        .map_err(|_| db_error())?
        .ok_or_else(|| AitError::unauthorized().into_response())?;

    let session_user = user.to_session_user();

    if session_user.role != UserRole::Admin {
        tracing::warn!(
            "Non-admin user '{}' (role: {:?}) accessing admin endpoint: {} {}",
            session_user.username,
            session_user.role,
            req.method(),
            req.uri().path()
        );
    }

    req.extensions_mut().insert(session_user);
    Ok(next.run(req).await)
}

fn get_client_ip(req: &Request) -> Option<IpAddr> {
    if let Some(value) = req.headers().get("x-forwarded-for")
        && let Ok(value) = value.to_str()
        && let Some(ip) = value.split(',').next().map(str::trim)
        && let Ok(ip) = ip.parse::<IpAddr>()
    {
        return Some(ip);
    }
    if let Some(value) = req.headers().get("x-real-ip")
        && let Ok(value) = value.to_str()
        && let Ok(ip) = value.parse::<IpAddr>()
    {
        return Some(ip);
    }
    if let Some(connect_info) = req.extensions().get::<ConnectInfo<SocketAddr>>() {
        return Some(connect_info.0.ip());
    }
    None
}

fn check_and_record(
    limiter: &crate::rate_limiter::RateLimiter,
    ip: IpAddr,
    max_attempts: u64,
    window_secs: u64,
    ban_secs: u64,
) -> Result<(), (StatusCode, Json<AitError>)> {
    limiter
        .check_and_record(ip, max_attempts, window_secs, ban_secs)
        .map_err(|_| too_many_requests("Too many attempts. Please try again later."))
}

pub async fn login_rate_limit_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    let config = &state.config.auth.login_rate_limit;
    let ip =
        get_client_ip(&req).ok_or_else(|| too_many_requests("Could not determine client IP"))?;

    check_and_record(
        &state.login_rate_limiter,
        ip,
        config.max_attempts,
        config.window_secs,
        config.ban_secs,
    )?;

    let response = next.run(req).await;

    if response.status() == StatusCode::OK {
        state.login_rate_limiter.clear(ip);
    }

    Ok(response)
}

pub async fn register_rate_limit_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    let config = &state.config.auth.register_rate_limit;
    let ip =
        get_client_ip(&req).ok_or_else(|| too_many_requests("Could not determine client IP"))?;

    check_and_record(
        &state.register_rate_limiter,
        ip,
        config.max_attempts,
        config.window_secs,
        config.ban_secs,
    )?;

    Ok(next.run(req).await)
}

pub async fn access_log_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let start = Instant::now();
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let username = req
        .extensions()
        .get::<SessionUser>()
        .map(|u| u.username.clone());

    let client_ip = get_client_ip(&req).map(|ip| ip.to_string());
    let response = next.run(req).await;

    let latency = start.elapsed();
    let status = response.status().as_u16() as i32;

    state.log_manager.log_access(AccessEvent {
        timestamp: Utc::now(),
        method,
        path,
        status,
        latency_ms: latency.as_millis() as i64,
        username,
        client_ip,
    });

    response
}
