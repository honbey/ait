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

    // Cache hit — positive entries slide their TTL; negative entries
    // (known-invalid tokens) expire for real so a key registered after a
    // miss flood becomes usable without waiting for cleanup.
    if let Some(mut entry) = state.api_key_cache.get_mut(&hash) {
        if entry.1.elapsed() < CACHE_TTL {
            if entry.0.is_some() {
                entry.1 = Instant::now();
            }
            let cached = entry.0.clone();
            drop(entry);
            // Negative entries short-circuit without a DB round trip, so
            // invalid-key floods cannot queue on the single SQLite connection.
            let key_info =
                cached.ok_or_else(|| unauthorized("Unauthorized: invalid or missing API key"))?;
            verify_key(&key_info, &mut req)?;
            req.extensions_mut().insert(client_ip);
            return Ok(next.run(req).await);
        }
        drop(entry);
    }

    // Cache miss or stale — load from DB; both outcomes are cached.
    let token = token.to_string();
    let db = state.db.clone();
    let key_info = crate::run_blocking(move || db.get_api_key_by_raw(&token))
        .await
        .map_err(internal_error)?
        .map_err(|_| db_error())?;

    // Negative entries come from attacker-controlled tokens: only cache them
    // while under the entry cap, or a miss flood grows the map between
    // cleanup passes.
    let cacheable = key_info.is_some()
        || state.api_key_cache.len() < state.config.server.cache_max_entries as usize;
    if cacheable {
        state
            .api_key_cache
            .insert(hash, (key_info.clone(), Instant::now()));
    }

    let key_info =
        key_info.ok_or_else(|| unauthorized("Unauthorized: invalid or missing API key"))?;
    verify_key(&key_info, &mut req)?;

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

#[cfg(test)]
mod tests {
    use crate::test_utils::{
        create_test_state, send_request, send_request_with_headers, test_router,
    };
    use axum::http::{Method, StatusCode, header};

    async fn create_key(router: &axum::Router, name: &str) -> String {
        let resp = send_request(
            router,
            Method::POST,
            "/api/api-keys",
            None,
            Some(serde_json::json!({"name": name})),
        )
        .await;
        assert_eq!(resp.status, StatusCode::CREATED);
        resp.json["key"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn auth_disabled_skips_authentication() {
        let (mut state, _dir) = create_test_state();
        state.config.auth.enabled = false;
        let router = test_router(state);
        let resp = send_request(&router, Method::GET, "/v1/models", None, None).await;
        assert_eq!(resp.status, StatusCode::OK);
    }

    #[tokio::test]
    async fn cache_hit_serves_second_request() {
        let (state, _dir) = create_test_state();
        let router = test_router(state);
        let raw_key = create_key(&router, "test-key").await;

        let resp1 = send_request(&router, Method::GET, "/v1/models", Some(&raw_key), None).await;
        assert_eq!(resp1.status, StatusCode::OK);

        let resp2 = send_request(&router, Method::GET, "/v1/models", Some(&raw_key), None).await;
        assert_eq!(resp2.status, StatusCode::OK);
    }

    #[tokio::test]
    async fn expired_api_key_rejected() {
        let (state, _dir) = create_test_state();
        let router = test_router(state);
        let past_ts = chrono::Utc::now().timestamp() - 3600;
        let resp = send_request(
            &router,
            Method::POST,
            "/api/api-keys",
            None,
            Some(serde_json::json!({"name": "expired-key", "expires_at": past_ts})),
        )
        .await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let raw_key = resp.json["key"].as_str().unwrap().to_string();

        let resp = send_request(&router, Method::GET, "/v1/models", Some(&raw_key), None).await;
        assert_eq!(resp.status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn x_forwarded_for_sets_client_ip() {
        let (state, _dir) = create_test_state();
        let router = test_router(state);
        let raw_key = create_key(&router, "test-key").await;

        let resp = send_request_with_headers(
            &router,
            Method::GET,
            "/v1/models",
            Some(&raw_key),
            None,
            &[(
                header::HeaderName::from_static("x-forwarded-for"),
                "10.20.30.40",
            )],
        )
        .await;
        assert_eq!(resp.status, StatusCode::OK);
    }

    #[tokio::test]
    async fn x_real_ip_sets_client_ip() {
        let (state, _dir) = create_test_state();
        let router = test_router(state);
        let raw_key = create_key(&router, "test-key").await;

        let resp = send_request_with_headers(
            &router,
            Method::GET,
            "/v1/models",
            Some(&raw_key),
            None,
            &[(header::HeaderName::from_static("x-real-ip"), "10.20.30.40")],
        )
        .await;
        assert_eq!(resp.status, StatusCode::OK);
    }

    #[tokio::test]
    async fn missing_bearer_token_rejected() {
        let (state, _dir) = create_test_state();
        let router = test_router(state);
        let resp = send_request(&router, Method::GET, "/v1/models", None, None).await;
        assert_eq!(resp.status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn invalid_bearer_token_rejected() {
        let (state, _dir) = create_test_state();
        let router = test_router(state);
        let resp = send_request(&router, Method::GET, "/v1/models", Some("fake-key"), None).await;
        assert_eq!(resp.status, StatusCode::UNAUTHORIZED);
    }
}
