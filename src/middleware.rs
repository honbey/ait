use crate::app::{AppState, NEGATIVE_CACHE_TTL};
use crate::config::TrustedProxy;
use crate::db::{AccessEvent, ApiKeyContext, ApiKeyInfo, RequestId, hash_key};
use crate::error::{AitError, REQUEST_ID, db_error, internal_error, unauthorized};
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
    let client_ip = get_client_ip(
        &req,
        &state.config.server.trusted_proxies,
        state.config.server.trusted_proxy_hops,
    );

    if !state.config.auth.enabled {
        req.extensions_mut().insert(ApiKeyContext { name: None });
        req.extensions_mut().insert(client_ip);
        return Ok(next.run(req).await);
    }

    let token = extract_bearer_token(req.headers())
        .ok_or_else(|| unauthorized("Unauthorized: invalid or missing API key"))?;

    let hash = hash_key(token);

    // Known-invalid token: short-circuit without a DB round trip. This map is
    // separate and bounded so a flood of distinct bogus tokens cannot evict
    // valid entries from `api_key_cache`, nor escape caching once the cap is
    // reached — either of which would send every flood request to the single
    // SQLite connection.
    if state
        .negative_key_cache
        .get(&hash)
        .is_some_and(|seen| seen.elapsed() < NEGATIVE_CACHE_TTL)
    {
        return Err(unauthorized("Unauthorized: invalid or missing API key"));
    }

    // Cache hit — every entry here is a valid key, so the TTL always slides.
    if let Some(mut entry) = state.api_key_cache.get_mut(&hash) {
        if entry.1.elapsed() < CACHE_TTL {
            entry.1 = Instant::now();
            let cached = entry.0.clone();
            drop(entry);
            verify_key(&cached, &mut req)?;
            req.extensions_mut().insert(client_ip);
            return Ok(next.run(req).await);
        }
        drop(entry);
    }

    // Cache miss or stale — load from DB. Valid keys share one cache; invalid
    // tokens go to the bounded negative map, which is capped so a flood of
    // distinct tokens cannot grow it without bound between cleanup passes.
    let token = token.to_string();
    let db = state.db.clone();
    // Hold a permit only for the DB call: a flood of distinct invalid tokens
    // parks here instead of exhausting the blocking pool that every other DB
    // operation depends on. Requests queue rather than fail closed.
    let permit = state
        .auth_lookup_permits
        .clone()
        .acquire_owned()
        .await
        .map_err(internal_error)?;
    let key_info = crate::run_blocking(move || db.get_api_key_by_raw(&token))
        .await
        .map_err(internal_error)?
        .map_err(|_| db_error())?;
    drop(permit);

    match key_info {
        Some(info) => {
            state
                .api_key_cache
                .insert(hash, (info.clone(), Instant::now()));
            verify_key(&info, &mut req)?;
        }
        None => {
            if state.negative_key_cache.len() < state.config.server.cache_max_entries as usize {
                state.negative_key_cache.insert(hash, Instant::now());
            }
            return Err(unauthorized("Unauthorized: invalid or missing API key"));
        }
    }

    req.extensions_mut().insert(client_ip);
    Ok(next.run(req).await)
}

fn is_trusted_proxy(ip: IpAddr, trusted: &[TrustedProxy]) -> bool {
    trusted.iter().any(|t| t.contains(ip))
}

fn get_client_ip(req: &Request, trusted_proxies: &[TrustedProxy], hops: usize) -> Option<IpAddr> {
    let direct_ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip());

    let trusted = direct_ip.is_some_and(|ip| is_trusted_proxy(ip, trusted_proxies));

    if trusted {
        if let Some(value) = req.headers().get("x-forwarded-for")
            && let Ok(value) = value.to_str()
            && let Some(ip) = forwarded_client_ip(value, hops)
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

/// Pick the address `hops` trusted proxies back in the X-Forwarded-For
/// chain. The nearest trusted proxy appends the peer it actually saw, so
/// the real client address sits `hops` entries from the right; leftmost
/// entries come from the client itself and are trivially spoofable.
/// Returns `None` when the chain is shorter than `hops` or the picked
/// entry is not a valid IP, letting the caller fall back to X-Real-IP or
/// the direct peer address.
fn forwarded_client_ip(value: &str, hops: usize) -> Option<IpAddr> {
    let parts: Vec<&str> = value.split(',').map(str::trim).collect();
    parts
        .len()
        .checked_sub(hops)
        .and_then(|i| parts.get(i))
        .and_then(|entry| entry.parse::<IpAddr>().ok())
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

    let client_ip = get_client_ip(
        &req,
        &state.config.server.trusted_proxies,
        state.config.server.trusted_proxy_hops,
    )
    .map(|ip| ip.to_string());
    req.extensions_mut().insert(RequestId(request_id.clone()));
    // Scope the id around the rest of the request so `AitError::into_response`
    // can stamp it into error bodies as they are built - no body rewriting.
    let mut response = REQUEST_ID.scope(request_id.clone(), next.run(req)).await;

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
    use super::{ConnectInfo, Request, forwarded_client_ip, get_client_ip};
    use crate::config::TrustedProxy;
    use crate::test_utils::{
        create_test_state, send_request, send_request_with_headers, test_router,
    };
    use axum::body::Body;
    use axum::http::{Method, StatusCode, header};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

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

    #[tokio::test]
    async fn error_body_carries_request_id() {
        let (state, _dir) = create_test_state();
        let router = test_router(state);
        let resp = send_request(&router, Method::GET, "/api/providers/nope", None, None).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
        let header_id = resp
            .headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        assert_eq!(resp.json["request_id"].as_str(), header_id.as_deref());
    }

    // ── X-Forwarded-For hop selection ──

    fn request_with_peer(peer: IpAddr, xff: Option<&str>) -> Request {
        let mut builder = Request::builder().method(Method::GET).uri("/");
        if let Some(xff) = xff {
            builder = builder.header("x-forwarded-for", xff);
        }
        let mut req = builder.body(Body::empty()).unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::new(peer, 0)));
        req
    }

    fn trusted() -> Vec<TrustedProxy> {
        vec![
            TrustedProxy::Ip(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            TrustedProxy::Ip(IpAddr::V6(Ipv6Addr::LOCALHOST)),
        ]
    }

    #[test]
    fn xff_takes_rightmost_entry_for_single_hop() {
        let req = request_with_peer(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            Some("6.6.6.6, 10.20.30.40"),
        );
        let expected = "10.20.30.40".parse().unwrap();
        assert_eq!(get_client_ip(&req, &trusted(), 1), Some(expected));
    }

    #[test]
    fn xff_picks_nth_entry_from_right_for_multi_hop() {
        let req = request_with_peer(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            Some("203.0.113.9, 10.0.0.1"),
        );
        let expected = "203.0.113.9".parse().unwrap();
        assert_eq!(get_client_ip(&req, &trusted(), 2), Some(expected));
    }

    #[test]
    fn xff_chain_shorter_than_hops_falls_back_to_peer() {
        let req = request_with_peer(IpAddr::V4(Ipv4Addr::LOCALHOST), Some("10.20.30.40"));
        let expected = IpAddr::V4(Ipv4Addr::LOCALHOST);
        assert_eq!(get_client_ip(&req, &trusted(), 3), Some(expected));
    }

    #[test]
    fn untrusted_peer_ignores_xff() {
        let req = request_with_peer("10.9.9.9".parse().unwrap(), Some("6.6.6.6"));
        let expected = "10.9.9.9".parse().unwrap();
        assert_eq!(get_client_ip(&req, &trusted(), 1), Some(expected));
    }

    #[test]
    fn cidr_trusted_proxy_enables_xff() {
        // An ingress inside a subnet cannot be listed by address; the whole
        // block has to be trusted for XFF to apply.
        let req = request_with_peer("10.0.0.7".parse().unwrap(), Some("203.0.113.9"));
        let trusted = vec![TrustedProxy::Cidr("10.0.0.0".parse().unwrap(), 8)];
        let expected = "203.0.113.9".parse().unwrap();
        assert_eq!(get_client_ip(&req, &trusted, 1), Some(expected));

        let outside = request_with_peer("11.0.0.7".parse().unwrap(), Some("203.0.113.9"));
        let expected = "11.0.0.7".parse().unwrap();
        assert_eq!(get_client_ip(&outside, &trusted, 1), Some(expected));
    }

    #[test]
    fn forwarded_client_ip_rejects_unusable_entries() {
        let ok = "10.20.30.40".parse().unwrap();
        // An unparseable picked entry yields None so the caller falls back
        // to X-Real-IP / the direct peer.
        assert_eq!(forwarded_client_ip("not-an-ip", 1), None);
        // hops = 0 ignores the chain entirely.
        assert_eq!(forwarded_client_ip("10.20.30.40", 0), None);
        assert_eq!(forwarded_client_ip(" 6.6.6.6 , 10.20.30.40 ", 1), Some(ok));
    }
}
