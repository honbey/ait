use crate::app::AppState;
use crate::db::{AccessEvent, ApiKeyInfo, SessionUser, hash_key};
use crate::error::{AitError, db_error, internal_error, too_many_requests, unauthorized};
use axum::{
    Json,
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::Response,
};
use chrono::Utc;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

pub(crate) const CACHE_TTL: Duration = Duration::from_secs(300);

fn full_access(username: &str) -> SessionUser {
    SessionUser {
        username: username.to_string(),
        api_key_name: None,
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

fn verify_key(
    key_info: &ApiKeyInfo,
    req: &mut Request,
) -> Result<(), (StatusCode, Json<AitError>)> {
    if !key_info.enabled {
        return Err(unauthorized("Unauthorized: invalid or missing API key"));
    }
    if key_info.expires_at.is_some_and(|exp| exp <= Utc::now()) {
        return Err(unauthorized("Unauthorized: invalid or missing API key"));
    }
    req.extensions_mut().insert(SessionUser {
        username: key_info.username.clone(),
        api_key_name: Some(key_info.name.clone()),
    });
    Ok(())
}

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    let client_ip = get_client_ip(&req, &state.config.server.trusted_proxies);

    if !state.config.auth.enabled {
        req.extensions_mut().insert(full_access("anonymous"));
        req.extensions_mut().insert(client_ip);
        return Ok(next.run(req).await);
    }

    let token = extract_bearer_token(req.headers())
        .ok_or_else(|| unauthorized("Unauthorized: invalid or missing API key"))?;

    let hash = hash_key(token);

    // Cache hit — verify freshness and renew TTL
    if let Some(mut entry) = state.api_key_cache.get_mut(&hash) {
        if entry.1.elapsed() < CACHE_TTL {
            entry.1 = Instant::now();
            let key_info = entry.0.clone();
            drop(entry);
            verify_key(&key_info, &mut req)?;
            req.extensions_mut().insert(client_ip);
            return Ok(next.run(req).await);
        }
        drop(entry);
    }

    // Cache miss or stale — load from DB
    let token = token.to_string();
    let db = state.db.clone();
    let key_info = crate::run_blocking(move || db.get_user_by_api_key(&token))
        .await
        .map_err(internal_error)?
        .map_err(|_| db_error())?
        .ok_or_else(|| unauthorized("Unauthorized: invalid or missing API key"))?;

    verify_key(&key_info, &mut req)?;

    state
        .api_key_cache
        .insert(hash, (key_info.clone(), Instant::now()));

    req.extensions_mut().insert(client_ip);
    Ok(next.run(req).await)
}

pub async fn admin_auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    let client_ip = get_client_ip(&req, &state.config.server.trusted_proxies);

    // Admin endpoints always require authentication
    let session_key = extract_session_key(req.headers())
        .ok_or_else(|| unauthorized("Unauthorized: invalid or missing API key"))?;

    let hash = hash_key(session_key);

    // Cache hit — verify freshness and renew TTL
    if let Some(mut entry) = state.session_cache.get_mut(&hash) {
        if entry.1 > Utc::now() && entry.2.elapsed() < CACHE_TTL {
            entry.2 = Instant::now();
            let user = entry.0.clone();
            drop(entry);
            req.extensions_mut().insert(user);
            req.extensions_mut().insert(client_ip);
            return Ok(next.run(req).await);
        }
        drop(entry);
    }

    // Cache miss or stale — load from DB (only session; username is enough for SessionUser)
    let session_key = session_key.to_string();
    let db = state.db.clone();
    let session = crate::run_blocking(move || db.get_session(&session_key))
        .await
        .map_err(internal_error)?
        .map_err(|_| db_error())?
        .ok_or_else(|| unauthorized("Unauthorized: invalid or missing API key"))?;

    if session.is_expired() {
        return Err(unauthorized("Unauthorized: invalid or missing API key"));
    }

    let user = SessionUser {
        username: session.username,
        api_key_name: None,
    };

    state
        .session_cache
        .insert(hash, (user.clone(), session.expires_at, Instant::now()));

    req.extensions_mut().insert(user);
    req.extensions_mut().insert(client_ip);
    Ok(next.run(req).await)
}

fn is_trusted_proxy(ip: IpAddr, trusted: &[IpAddr]) -> bool {
    trusted.contains(&ip)
}

fn get_client_ip(req: &Request, trusted_proxies: &[IpAddr]) -> Option<IpAddr> {
    let direct_ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip());

    let trusted = direct_ip.is_some_and(|ip| is_trusted_proxy(ip, trusted_proxies));

    if trusted {
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
    }

    direct_ip
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
    let ip = get_client_ip(&req, &state.config.server.trusted_proxies)
        .ok_or_else(|| too_many_requests("Could not determine client IP"))?;

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

    let client_ip =
        get_client_ip(&req, &state.config.server.trusted_proxies).map(|ip| ip.to_string());
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
