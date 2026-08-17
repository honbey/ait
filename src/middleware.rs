use crate::app::AppState;
use crate::db::{AccessEvent, ApiKeyContext, ApiKeyInfo, RequestId, hash_key};
use crate::error::{AitError, db_error, internal_error, unauthorized};
use axum::{
    Json,
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    middleware::Next,
    response::Response,
};
use chrono::Utc;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};
use uuid::Uuid;

pub(crate) const CACHE_TTL: Duration = Duration::from_secs(300);

/// Extract Bearer token from Authorization header.
fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
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
    req.extensions_mut().insert(ApiKeyContext {
        name: Some(key_info.name.clone()),
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
        req.extensions_mut().insert(ApiKeyContext { name: None });
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

pub async fn access_log_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let start = Instant::now();
    let request_id = Uuid::new_v4().to_string();
    let method = req.method().to_string();
    let path = req.uri().path().to_string();

    let client_ip =
        get_client_ip(&req, &state.config.server.trusted_proxies).map(|ip| ip.to_string());
    req.extensions_mut().insert(RequestId(request_id.clone()));
    let mut response = next.run(req).await;

    response.headers_mut().insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_str(&request_id).expect("UUID is valid ASCII"),
    );

    let latency = start.elapsed();
    let status = response.status().as_u16() as i32;

    state.log_manager.log_access(AccessEvent {
        timestamp: Utc::now(),
        request_id,
        method,
        path,
        status,
        latency_ms: latency.as_millis() as i64,
        client_ip,
    });

    response
}
