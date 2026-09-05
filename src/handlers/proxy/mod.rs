use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Instant;

use axum::{
    Extension, Json as AxumJson,
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use tracing::{debug, trace};

use crate::app::{AppState, NEGATIVE_CACHE_TTL};
use crate::db::{ApiKeyContext, Model, Provider, ProxyEvent, RequestId};
use crate::error::{AitError, internal_error, not_found};
use crate::middleware::CACHE_TTL;
use crate::providers::create_provider;
use crate::ssrf;
use crate::utils::mask_sensitive_value;

/// Cache entries that know when they were written.
///
/// `insert_capped` is generic over the value type, so it cannot reach a
/// timestamp on its own; the caches that go through it expose theirs here.
trait TimestampedEntry {
    fn inserted_at(&self) -> Instant;
}

impl TimestampedEntry for Instant {
    fn inserted_at(&self) -> Instant {
        *self
    }
}

impl<T> TimestampedEntry for (T, Instant) {
    fn inserted_at(&self) -> Instant {
        self.1
    }
}

/// Insert into a cache, evicting the oldest entry once the cap is reached.
///
/// Dropping the new entry instead would stop caching altogether: hot entries
/// slide their timestamp on every hit (see `proxy_request`), so they never age
/// out during cleanup and the cache stays pinned at the cap forever.
///
/// `DashMap::len` is approximate under concurrency, so a cache stays bounded
/// within a small multiple of `cache_max_entries`.
///
/// Shared by the model cache and the negative model cache (keys come from
/// request bodies and are therefore attacker-controlled) and the provider
/// cache, so all three enforce the cap.
fn insert_capped<V: TimestampedEntry>(
    map: &dashmap::DashMap<String, V>,
    key: &str,
    entry: V,
    max_entries: usize,
) {
    if max_entries == 0 {
        return;
    }
    // Refreshing an existing key must not count against the cap.
    if map.contains_key(key) {
        map.insert(key.to_string(), entry);
        return;
    }
    if map.len() >= max_entries {
        // Clone the key and let the iterator (and the shard read lock it
        // holds) go before removing; parking_lot's RwLock is not reentrant.
        let oldest = map
            .iter()
            .min_by_key(|e| e.value().inserted_at())
            .map(|e| e.key().clone());
        if let Some(oldest) = oldest {
            map.remove(&oldest);
        }
    }
    map.insert(key.to_string(), entry);
}

mod exec;
mod guard;
mod sse;

use exec::{proxy_non_streamed, proxy_streamed};
use guard::{ProxyLogGuard, UsageTokens};

/// Parse a request body and release the raw bytes immediately.
///
/// The `Bytes` holding the request is only needed while parsing, but a
/// shadowed binding keeps it alive until the handler returns — that is the
/// whole upstream round-trip, which for a stream can be minutes. Releasing it
/// here keeps a copy of the body (up to `max_request_body_bytes`) out of
/// memory for the duration, per in-flight request.
fn parse_body(
    body: bytes::Bytes,
) -> Result<(serde_json::Value, usize), (StatusCode, Json<AitError>)> {
    let body_len = body.len();
    let parsed = serde_json::from_slice(&body)
        .map_err(|_| AitError::bad_request("invalid request body").into_response())?;
    drop(body);
    Ok((parsed, body_len))
}

pub async fn chat_completions(
    State(state): State<AppState>,
    Extension(api_key): Extension<ApiKeyContext>,
    Extension(client_ip): Extension<Option<IpAddr>>,
    Extension(request_id): Extension<RequestId>,
    body: bytes::Bytes,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    let (body, body_len) = parse_body(body)?;
    proxy_request(
        state,
        api_key,
        client_ip,
        request_id,
        body,
        body_len,
        "/chat/completions",
    )
    .await
}

pub async fn completions(
    State(state): State<AppState>,
    Extension(api_key): Extension<ApiKeyContext>,
    Extension(client_ip): Extension<Option<IpAddr>>,
    Extension(request_id): Extension<RequestId>,
    body: bytes::Bytes,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    let (body, body_len) = parse_body(body)?;
    proxy_request(
        state,
        api_key,
        client_ip,
        request_id,
        body,
        body_len,
        "/completions",
    )
    .await
}

pub async fn embeddings(
    State(state): State<AppState>,
    Extension(api_key): Extension<ApiKeyContext>,
    Extension(client_ip): Extension<Option<IpAddr>>,
    Extension(request_id): Extension<RequestId>,
    body: bytes::Bytes,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    let (body, body_len) = parse_body(body)?;
    proxy_request(
        state,
        api_key,
        client_ip,
        request_id,
        body,
        body_len,
        "/embeddings",
    )
    .await
}

pub async fn responses(
    State(state): State<AppState>,
    Extension(api_key): Extension<ApiKeyContext>,
    Extension(client_ip): Extension<Option<IpAddr>>,
    Extension(request_id): Extension<RequestId>,
    body: bytes::Bytes,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    let (body, body_len) = parse_body(body)?;
    proxy_request(
        state,
        api_key,
        client_ip,
        request_id,
        body,
        body_len,
        "/responses",
    )
    .await
}

pub async fn health(State(state): State<AppState>) -> AxumJson<serde_json::Value> {
    if state.config.server.health_detail {
        let uptime = Utc::now() - state.start_time;
        let total_secs = uptime.num_seconds().max(0) as u64;
        let hours = total_secs / 3600;
        let mins = (total_secs % 3600) / 60;
        let secs = total_secs % 60;

        AxumJson(serde_json::json!({
            "status": "ok",
            "version": env!("CARGO_PKG_VERSION"),
            "uptime": format!("{}h{}m{}s", hours, mins, secs),
            "uptime_secs": total_secs,
            "start_time": state.start_time.timestamp(),
        }))
    } else {
        AxumJson(serde_json::json!({
            "status": "ok"
        }))
    }
}

pub async fn list_models_proxy(
    State(state): State<AppState>,
    Extension(_api_key): Extension<ApiKeyContext>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AitError>)> {
    let db = state.db.clone();
    let (models, providers) = crate::run_blocking(move || {
        let m = db.list_models()?;
        let p = db.list_providers()?;
        Ok::<_, crate::db::DbError>((m, p))
    })
    .await
    .map_err(internal_error)?
    .map_err(internal_error)?;

    let provider_names: HashMap<&str, &str> = providers
        .iter()
        .map(|p| (p.id.as_str(), p.name.as_str()))
        .collect();

    let data: Vec<serde_json::Value> = models
        .into_iter()
        .filter(|m| m.enabled)
        .map(|m| {
            let owned_by = provider_names
                .get(m.provider_id.as_str())
                .copied()
                .unwrap_or("unknown");
            serde_json::json!({
                "id": m.name,
                "object": "model",
                "created": m.created_at.timestamp(),
                "owned_by": owned_by,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "object": "list",
        "data": data,
    })))
}

pub async fn proxy_request(
    state: AppState,
    api_key: ApiKeyContext,
    client_ip: Option<IpAddr>,
    request_id: RequestId,
    body: serde_json::Value,
    body_len: usize,
    upstream_path: &str,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    let start = Instant::now();

    // Matches the transport layer: `0` disables the cap entirely rather than
    // rejecting every non-empty body.
    let max_request_body = state.config.proxy.max_request_body_bytes;
    if max_request_body > 0 && body_len as u64 > max_request_body {
        return Err(AitError::bad_request("request body exceeds max allowed size").into_response());
    }

    if state.dlp.is_enabled()
        && let Some(value) = state.dlp.scan(&body)
    {
        let reason = format!(
            "blocked by DLP: sensitive value '{}' matched",
            mask_sensitive_value(value, 3, 2)
        );
        state.log_manager.log_proxy(ProxyEvent {
            timestamp: Utc::now(),
            request_id: request_id.0,
            api_key_name: api_key.name.clone(),
            model_name: body
                .get("model")
                .and_then(|m| m.as_str())
                .unwrap_or("blocked")
                .to_string(),
            provider_name: "unknown".to_string(),
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            cached_tokens: None,
            latency_ms: start.elapsed().as_millis() as i64,
            status: "400".to_string(),
            endpoint: upstream_path.to_string(),
            is_streaming: false,
            time_to_first_token_ms: None,
            upstream_model: "blocked".to_string(),
            provider_type: "unknown".to_string(),
            response_body_size: None,
            error_message: Some(reason),
            client_ip: client_ip.map(|ip| ip.to_string()),
        });
        return Err(
            AitError::bad_request("request blocked by sensitive data rule").into_response(),
        );
    }

    let model_name = body.get("model").and_then(|m| m.as_str()).ok_or_else(|| {
        AitError::bad_request("Missing 'model' field in request body").into_response()
    })?;

    trace!(model_name, "proxy_request: start");

    let (model, provider) = {
        async fn resolve(
            state: &AppState,
            model_name: &str,
        ) -> Result<(Model, Provider), (StatusCode, Json<AitError>)> {
            let db = state.db.clone();
            let name = model_name.to_string();
            let max_entries = state.config.server.cache_max_entries as usize;
            match crate::run_blocking(move || db.resolve_model(&name)).await {
                Ok(Ok(Some((m, p)))) => {
                    let upstream = create_provider(&p, state.http_client.clone());
                    insert_capped(
                        &state.provider_cache,
                        &p.id,
                        (upstream, Instant::now()),
                        max_entries,
                    );
                    insert_capped(
                        &state.model_cache,
                        model_name,
                        ((m.clone(), p.clone()), Instant::now()),
                        max_entries,
                    );
                    Ok((m, p))
                }
                Ok(Ok(None)) => {
                    // Unknown models go to their own bounded cache: sharing
                    // `model_cache` let a flood of bogus names fill the entry
                    // cap, after which valid models stopped being cached too.
                    insert_capped(
                        &state.negative_model_cache,
                        model_name,
                        Instant::now(),
                        max_entries,
                    );
                    Err(not_found(format!(
                        "Model '{}' not found or disabled",
                        model_name
                    )))
                }
                Ok(Err(e)) => Err(AitError::from_db_error(e).into_response()),
                Err(join_err) => Err(internal_error(join_err)),
            }
        }

        // Known-unknown first: it costs one lookup and keeps the negative
        // verdict out of `model_cache` entirely.
        let negative = state
            .negative_model_cache
            .get(model_name)
            .is_some_and(|seen| seen.elapsed() < NEGATIVE_CACHE_TTL);
        if negative {
            return Err(not_found(format!(
                "Model '{}' not found or disabled",
                model_name
            )));
        }

        let cached = state.model_cache.get_mut(model_name).and_then(|mut entry| {
            if entry.1.elapsed() < CACHE_TTL {
                // slide positive entries so hot models stay cached
                entry.1 = Instant::now();
                Some(entry.0.clone())
            } else {
                None
            }
        });
        match cached {
            Some((m, p)) => (m, p),
            None => resolve(&state, model_name).await?,
        }
    };

    trace!(
        model = model_name,
        provider = provider.name,
        "proxy_request: model resolved, elapsed={}ms",
        start.elapsed().as_millis()
    );

    if !provider.provider_type.supports_endpoint(upstream_path) {
        return Err(AitError::bad_request(format!(
            "provider type '{}' does not support endpoint '{}'",
            provider.provider_type.as_ref(),
            upstream_path
        ))
        .into_response());
    }

    let max_entries = state.config.server.cache_max_entries as usize;
    let cached_upstream = state
        .provider_cache
        .get_mut(&provider.id)
        .and_then(|mut entry| {
            if entry.1.elapsed() < CACHE_TTL {
                entry.1 = Instant::now();
                Some(entry.0.clone())
            } else {
                None
            }
        });
    let upstream = match cached_upstream {
        Some(upstream) => upstream,
        None => {
            let upstream = create_provider(&provider, state.http_client.clone());
            insert_capped(
                &state.provider_cache,
                &provider.id,
                (upstream.clone(), Instant::now()),
                max_entries,
            );
            upstream
        }
    };

    let model_name = model.name.clone();
    let provider_name = provider.name.clone();

    let stream = body
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false)
        && state.config.proxy.stream;

    let prompt_tokens = count_prompt_tokens(body_len, state.config.proxy.prompt_token_divisor);

    let request = upstream
        .build_request(
            &state.http_client,
            body,
            stream,
            &model.upstream_model,
            upstream_path,
        )
        .await
        .map_err(|e| AitError::bad_request(e).into_response())?;

    let verified_ips = ssrf::check_ssrf(
        request.url(),
        &state.config.security.ssrf_allowed_cidrs,
        &state.ssrf_dns_cache,
        &provider_name,
    )
    .await?;
    // Execute against a client whose DNS is pinned to the verified IPs, so
    // the connection cannot be re-resolved to a different address between
    // check and connect (DNS rebinding).
    let pinned = ssrf::pinned_client(&state, request.url(), &verified_ips)?;

    debug!(
        "Proxying to provider '{}' for model '{}' -> upstream '{}', base_url: {}",
        provider_name,
        model_name,
        model.upstream_model,
        request.url()
    );

    let endpoint = upstream_path.to_string();
    let upstream_model = model.upstream_model.clone();
    let provider_type = provider.provider_type.as_ref().to_string();
    let client_ip_str = client_ip.map(|ip| ip.to_string());

    if stream {
        trace!(
            model = model_name,
            provider = provider_name,
            "proxy_request: -> streamed, elapsed={}ms",
            start.elapsed().as_millis()
        );
        return proxy_streamed(
            state,
            pinned,
            request,
            upstream,
            provider_name,
            model_name,
            api_key.clone(),
            start,
            endpoint,
            upstream_model,
            provider_type,
            client_ip_str,
            prompt_tokens,
            request_id,
        )
        .await
        .map(|r| r.into_response());
    }

    let log_manager = state.log_manager.clone();
    let mut guard = ProxyLogGuard::new(
        log_manager,
        ProxyEvent {
            timestamp: Utc::now(),
            request_id: request_id.0,
            api_key_name: api_key.name.clone(),
            model_name: model_name.clone(),
            provider_name: provider_name.clone(),
            prompt_tokens,
            completion_tokens: None,
            total_tokens: None,
            cached_tokens: None,
            latency_ms: 0,
            status: "pending".to_string(),
            endpoint,
            is_streaming: false,
            time_to_first_token_ms: None,
            upstream_model,
            provider_type,
            response_body_size: None,
            error_message: None,
            client_ip: client_ip_str,
        },
        start,
    );

    trace!(
        model = model_name,
        provider = provider_name,
        "proxy_request: -> non-streamed, elapsed={}ms",
        start.elapsed().as_millis()
    );

    let result = proxy_non_streamed(
        state.clone(),
        pinned,
        request,
        upstream,
        &model_name,
        &provider,
        start,
    )
    .await;

    match result {
        Ok((resp, usage, ttfb, body_size)) => {
            trace!(
                model = model_name,
                provider = provider_name,
                "proxy_request: non-streamed done, ttfb={}ms, elapsed={}ms",
                ttfb,
                start.elapsed().as_millis()
            );
            guard.event.time_to_first_token_ms = Some(ttfb);
            guard.event.response_body_size = Some(body_size as i64);
            guard.finalize(&usage, "200");
            Ok(resp.into_response())
        }
        Err(e) => {
            trace!(
                model = model_name,
                provider = provider_name,
                "proxy_request: non-streamed error, elapsed={}ms",
                start.elapsed().as_millis()
            );
            guard.event.error_message = Some(
                e.1.0
                    .detail
                    .clone()
                    .unwrap_or_else(|| e.1.0.message.clone()),
            );
            guard.finalize(&UsageTokens::default(), &e.0.as_u16().to_string());
            Err(e)
        }
    }
}

/// Normal paths will be overwritten by the upstream usage precise value;
/// this fallback value is only used if the connection is interrupted.
/// `body_len` covers the whole JSON request (field names, base64 images and
/// all), so the divisor is configurable (`proxy.prompt_token_divisor`, 1-5)
/// to trim the estimate toward observed usage.
fn count_prompt_tokens(body_len: usize, divisor: u64) -> Option<i64> {
    if body_len == 0 {
        None
    } else {
        Some(body_len as i64 / divisor as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{ApiKeyContext, RequestId};
    use crate::providers::UpstreamProvider;
    use crate::test_utils::{
        create_test_provider, create_test_state, create_test_state_dlp, mock_upstream_redirect,
        mock_upstream_server, mock_upstream_sse_server, seed_provider_and_model, send_request,
        test_router,
    };
    use axum::Extension;
    use axum::http::Method;
    use bytes::Bytes;
    use std::sync::Arc;

    #[test]
    fn insert_capped_respects_cap() {
        let map: dashmap::DashMap<String, Instant> = dashmap::DashMap::new();
        let entry = Instant::now();
        for i in 0..5 {
            insert_capped(&map, &format!("m{i}"), entry, 5);
        }
        assert_eq!(map.len(), 5);
        insert_capped(&map, "m6", entry, 5);
        assert_eq!(map.len(), 5);
    }

    #[test]
    fn insert_capped_zero_cap_caches_nothing() {
        let map: dashmap::DashMap<String, Instant> = dashmap::DashMap::new();
        insert_capped(&map, "m0", Instant::now(), 0);
        assert!(map.is_empty());
    }

    #[test]
    fn insert_capped_evicts_oldest_when_full() {
        use std::time::Duration;

        let map: dashmap::DashMap<String, Instant> = dashmap::DashMap::new();
        let start = Instant::now();
        for i in 0..3 {
            insert_capped(&map, &format!("m{i}"), start + Duration::from_secs(i), 3);
        }
        // A full cache must keep caching: the new entry replaces the oldest
        // instead of being dropped, which is what kept caching off entirely.
        insert_capped(&map, "m3", start + Duration::from_secs(3), 3);
        assert_eq!(map.len(), 3);
        assert!(!map.contains_key("m0"), "oldest entry is evicted");
        assert!(map.contains_key("m3"), "newest entry is kept");
    }

    #[test]
    fn insert_capped_bounds_provider_cache() {
        struct NoopProvider;

        #[async_trait::async_trait]
        impl UpstreamProvider for NoopProvider {
            async fn build_request(
                &self,
                _client: &reqwest::Client,
                _body: serde_json::Value,
                _stream: bool,
                _upstream_model: &str,
                _upstream_path: &str,
            ) -> Result<reqwest::Request, String> {
                Err("noop provider is not usable".to_string())
            }
        }

        let map: dashmap::DashMap<String, (Arc<dyn UpstreamProvider>, Instant)> =
            dashmap::DashMap::new();
        let entry = || {
            (
                Arc::new(NoopProvider) as Arc<dyn UpstreamProvider>,
                Instant::now(),
            )
        };
        for i in 0..5 {
            insert_capped(&map, &format!("p{i}"), entry(), 5);
        }
        assert_eq!(map.len(), 5);
        insert_capped(&map, "p6", entry(), 5);
        assert_eq!(map.len(), 5, "provider cache must honour the same cap");
    }

    fn api_key() -> ApiKeyContext {
        ApiKeyContext {
            name: Some("test-key".to_string()),
        }
    }

    #[tokio::test]
    async fn dlp_blocks_request_containing_sensitive_value() {
        let (state, _dir) = create_test_state_dlp(&["13800138000"]);
        let body = serde_json::json!({
            "model": "x",
            "messages": [{"role": "user", "content": "my phone is 13800138000"}]
        });
        let result = chat_completions(
            State(state),
            Extension(api_key()),
            Extension(Some("127.0.0.1".parse().unwrap())),
            Extension(RequestId("r1".to_string())),
            Bytes::from(serde_json::to_vec(&body).unwrap()),
        )
        .await;
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn dlp_allows_request_without_sensitive_value() {
        let (state, _dir) = create_test_state_dlp(&["13800138000"]);
        let body = serde_json::json!({
            "model": "x",
            "messages": [{"role": "user", "content": "no sensitive data here"}]
        });
        let result = chat_completions(
            State(state),
            Extension(api_key()),
            Extension(Some("127.0.0.1".parse().unwrap())),
            Extension(RequestId("r2".to_string())),
            Bytes::from(serde_json::to_vec(&body).unwrap()),
        )
        .await;
        // DLP passes; the request fails later (unknown model -> 404), not at 400.
        let (status, _) = result.unwrap_err();
        assert_ne!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn dlp_request_size_limit_blocks_oversized_body() {
        let (mut state, _dir) = create_test_state_dlp(&[]);
        // Use a small limit so the test does not allocate a multi-megabyte body.
        state.config.proxy.max_request_body_bytes = 16;
        let body = serde_json::json!({ "model": "x", "content": "x".repeat(17) });
        let result = chat_completions(
            State(state),
            Extension(api_key()),
            Extension(Some("127.0.0.1".parse().unwrap())),
            Extension(RequestId("r3".to_string())),
            Bytes::from(serde_json::to_vec(&body).unwrap()),
        )
        .await;
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    // ── Mock upstream integration tests ──

    #[tokio::test]
    async fn proxy_non_streamed_happy_path() {
        let (state, _dir) = create_test_state();
        let (base_url, _captured) = mock_upstream_server(
            serde_json::json!({
                "id": "chatcmpl-1",
                "choices": [{"message": {"role": "assistant", "content": "hello"}}],
                "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
            }),
            axum::http::StatusCode::OK,
        );
        let raw_key = seed_provider_and_model(
            &state,
            crate::db::ProviderType::OpenAICompat,
            &base_url,
            "test-model",
        );
        let router = test_router(state.clone());
        let resp = send_request(
            &router,
            Method::POST,
            "/v1/chat/completions",
            Some(&raw_key),
            Some(serde_json::json!({
                "model": "test-model",
                "messages": [{"role": "user", "content": "hi"}],
            })),
        )
        .await;
        assert_eq!(resp.status, axum::http::StatusCode::OK);
        assert_eq!(resp.json["model"], "test-model");
        assert_eq!(resp.json["system_fingerprint"], "ait-proxy");
    }

    #[tokio::test]
    async fn proxy_model_not_found_returns_404() {
        let (state, _dir) = create_test_state();
        let raw_key = seed_provider_and_model(
            &state,
            crate::db::ProviderType::OpenAICompat,
            "http://127.0.0.1:1",
            "test-model",
        );
        let router = test_router(state);
        let resp = send_request(
            &router,
            Method::POST,
            "/v1/chat/completions",
            Some(&raw_key),
            Some(serde_json::json!({
                "model": "nonexistent-model",
                "messages": [{"role": "user", "content": "hi"}],
            })),
        )
        .await;
        assert_eq!(resp.status, axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn bogus_model_names_do_not_evict_resolved_models() {
        // Unknown names must land in `negative_model_cache` only: when they
        // shared `model_cache`, filling the cap with bogus names stopped
        // valid models from being cached at all.
        let (mut state, _dir) = create_test_state();
        let (base_url, _captured) = mock_upstream_server(
            serde_json::json!({
                "id": "chatcmpl-1",
                "choices": [{"message": {"role": "assistant", "content": "hello"}}],
                "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
            }),
            axum::http::StatusCode::OK,
        );
        let raw_key = seed_provider_and_model(
            &state,
            crate::db::ProviderType::OpenAICompat,
            &base_url,
            "test-model",
        );
        state.config.server.cache_max_entries = 4;
        let router = test_router(state.clone());

        for i in 0..4 {
            let resp = send_request(
                &router,
                Method::POST,
                "/v1/chat/completions",
                Some(&raw_key),
                Some(serde_json::json!({
                    "model": format!("bogus-{i}"),
                    "messages": [{"role": "user", "content": "hi"}],
                })),
            )
            .await;
            assert_eq!(resp.status, axum::http::StatusCode::NOT_FOUND);
        }

        let resp = send_request(
            &router,
            Method::POST,
            "/v1/chat/completions",
            Some(&raw_key),
            Some(serde_json::json!({
                "model": "test-model",
                "messages": [{"role": "user", "content": "hi"}],
            })),
        )
        .await;
        assert_eq!(resp.status, axum::http::StatusCode::OK);

        assert!(
            state.model_cache.contains_key("test-model"),
            "the resolved model must still be cached"
        );
        assert_eq!(state.model_cache.len(), 1);
        assert_eq!(state.negative_model_cache.len(), 4);
    }

    #[tokio::test]
    async fn creating_a_model_clears_its_negative_cache_entry() {
        // A name requested before it existed must not stay 404 after it is
        // created; the negative entry would outlive it by up to its TTL.
        let (state, _dir) = create_test_state();
        let (base_url, _captured) = mock_upstream_server(
            serde_json::json!({
                "id": "chatcmpl-1",
                "choices": [{"message": {"role": "assistant", "content": "hello"}}],
                "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
            }),
            axum::http::StatusCode::OK,
        );
        // A UUID id: the create-model endpoint rejects anything else.
        let provider = create_test_provider(
            "550e8400-e29b-41d4-a716-446655440000",
            crate::db::ProviderType::OpenAICompat,
            &base_url,
        );
        state.db.insert_provider(provider).unwrap();
        let (_stored, raw_key) = state.db.insert_api_key("test-key", None).unwrap();
        let router = test_router(state.clone());

        let resp = send_request(
            &router,
            Method::POST,
            "/v1/chat/completions",
            Some(&raw_key),
            Some(serde_json::json!({
                "model": "late-model",
                "messages": [{"role": "user", "content": "hi"}],
            })),
        )
        .await;
        assert_eq!(resp.status, axum::http::StatusCode::NOT_FOUND);
        assert!(state.negative_model_cache.contains_key("late-model"));

        let created = send_request(
            &router,
            Method::POST,
            "/api/models",
            None,
            Some(serde_json::json!({
                "name": "late-model",
                "provider_id": "550e8400-e29b-41d4-a716-446655440000",
                "upstream_model": "upstream-model",
                "enabled": true,
            })),
        )
        .await;
        assert_eq!(created.status, axum::http::StatusCode::CREATED);

        let resp = send_request(
            &router,
            Method::POST,
            "/v1/chat/completions",
            Some(&raw_key),
            Some(serde_json::json!({
                "model": "late-model",
                "messages": [{"role": "user", "content": "hi"}],
            })),
        )
        .await;
        assert_eq!(resp.status, axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn proxy_missing_model_field_returns_400() {
        let (state, _dir) = create_test_state();
        let raw_key = seed_provider_and_model(
            &state,
            crate::db::ProviderType::OpenAICompat,
            "http://127.0.0.1:1",
            "test-model",
        );
        let router = test_router(state);
        let resp = send_request(
            &router,
            Method::POST,
            "/v1/chat/completions",
            Some(&raw_key),
            Some(serde_json::json!({"messages": [{"role": "user", "content": "hi"}]})),
        )
        .await;
        assert_eq!(resp.status, axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn proxy_upstream_error_returns_upstream_status() {
        let (state, _dir) = create_test_state();
        let (base_url, _captured) = mock_upstream_server(
            serde_json::json!({"error": "rate limited"}),
            axum::http::StatusCode::TOO_MANY_REQUESTS,
        );
        let raw_key = seed_provider_and_model(
            &state,
            crate::db::ProviderType::OpenAICompat,
            &base_url,
            "test-model",
        );
        let router = test_router(state);
        let resp = send_request(
            &router,
            Method::POST,
            "/v1/chat/completions",
            Some(&raw_key),
            Some(serde_json::json!({
                "model": "test-model",
                "messages": [{"role": "user", "content": "hi"}],
            })),
        )
        .await;
        assert_eq!(resp.status, axum::http::StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn proxy_upstream_redirect_returns_502() {
        let (state, _dir) = create_test_state();
        let base_url = mock_upstream_redirect();
        let raw_key = seed_provider_and_model(
            &state,
            crate::db::ProviderType::OpenAICompat,
            &base_url,
            "test-model",
        );
        let router = test_router(state);
        let resp = send_request(
            &router,
            Method::POST,
            "/v1/chat/completions",
            Some(&raw_key),
            Some(serde_json::json!({
                "model": "test-model",
                "messages": [{"role": "user", "content": "hi"}],
            })),
        )
        .await;
        assert_eq!(resp.status, axum::http::StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn proxy_upstream_connection_failure_returns_502() {
        let (state, _dir) = create_test_state();
        let raw_key = seed_provider_and_model(
            &state,
            crate::db::ProviderType::OpenAICompat,
            "http://127.0.0.1:1",
            "test-model",
        );
        let router = test_router(state);
        let resp = send_request(
            &router,
            Method::POST,
            "/v1/chat/completions",
            Some(&raw_key),
            Some(serde_json::json!({
                "model": "test-model",
                "messages": [{"role": "user", "content": "hi"}],
            })),
        )
        .await;
        assert_eq!(resp.status, axum::http::StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn proxy_streamed_happy_path() {
        let (state, _dir) = create_test_state();
        let base_url = mock_upstream_sse_server(vec![
            r#"data: {"choices":[{"delta":{"content":"hello"}}]}"#.to_string(),
            r#"data: {"choices":[{"delta":{"content":" world"}}]}"#.to_string(),
        ]);
        let raw_key = seed_provider_and_model(
            &state,
            crate::db::ProviderType::OpenAICompat,
            &base_url,
            "test-model",
        );
        let router = test_router(state);
        let resp = send_request(
            &router,
            Method::POST,
            "/v1/chat/completions",
            Some(&raw_key),
            Some(serde_json::json!({
                "model": "test-model",
                "stream": true,
                "messages": [{"role": "user", "content": "hi"}],
            })),
        )
        .await;
        assert_eq!(resp.status, axum::http::StatusCode::OK);
        assert_eq!(
            resp.headers.get("content-type").unwrap(),
            "text/event-stream"
        );
    }

    #[tokio::test]
    async fn list_models_proxy_returns_enabled_models() {
        let (state, _dir) = create_test_state();
        seed_provider_and_model(
            &state,
            crate::db::ProviderType::OpenAICompat,
            "http://127.0.0.1:1",
            "test-model",
        );
        let router = test_router(state);
        let resp = send_request(&router, Method::GET, "/v1/models", None, None).await;
        assert_eq!(resp.status, axum::http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn list_models_proxy_with_key_returns_enabled_models() {
        let (state, _dir) = create_test_state();
        let raw_key = seed_provider_and_model(
            &state,
            crate::db::ProviderType::OpenAICompat,
            "http://127.0.0.1:1",
            "test-model",
        );
        let router = test_router(state);
        let resp = send_request(&router, Method::GET, "/v1/models", Some(&raw_key), None).await;
        assert_eq!(resp.status, axum::http::StatusCode::OK);
        let data = resp.json["data"]
            .as_array()
            .expect("data should be an array");
        assert_eq!(data.len(), 1);
        assert_eq!(data[0]["id"], "test-model");
        assert_eq!(data[0]["object"], "model");
    }

    #[tokio::test]
    async fn health_returns_ok_status() {
        let (state, _dir) = create_test_state();
        let router = test_router(state);
        let resp = send_request(&router, Method::GET, "/health", None, None).await;
        assert_eq!(resp.status, axum::http::StatusCode::OK);
        assert_eq!(resp.json["status"], "ok");
    }

    #[tokio::test]
    async fn health_detail_returns_version_and_uptime() {
        let (mut state, _dir) = create_test_state();
        state.config.server.health_detail = true;
        let router = test_router(state);
        let resp = send_request(&router, Method::GET, "/health", None, None).await;
        assert_eq!(resp.status, axum::http::StatusCode::OK);
        assert_eq!(resp.json["status"], "ok");
        assert!(resp.json.get("version").is_some());
        assert!(resp.json.get("uptime_secs").is_some());
    }

    #[tokio::test]
    async fn proxy_cache_hit_serves_second_request() {
        let (state, _dir) = create_test_state();
        let (base_url, _captured) = mock_upstream_server(
            serde_json::json!({
                "id": "chatcmpl-1",
                "choices": [{"message": {"role": "assistant", "content": "hello"}}],
                "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
            }),
            axum::http::StatusCode::OK,
        );
        let raw_key = seed_provider_and_model(
            &state,
            crate::db::ProviderType::OpenAICompat,
            &base_url,
            "test-model",
        );
        let router = test_router(state);
        let body = serde_json::json!({
            "model": "test-model",
            "messages": [{"role": "user", "content": "hi"}],
        });
        let resp1 = send_request(
            &router,
            Method::POST,
            "/v1/chat/completions",
            Some(&raw_key),
            Some(body.clone()),
        )
        .await;
        assert_eq!(resp1.status, axum::http::StatusCode::OK);
        let resp2 = send_request(
            &router,
            Method::POST,
            "/v1/chat/completions",
            Some(&raw_key),
            Some(body),
        )
        .await;
        assert_eq!(resp2.status, axum::http::StatusCode::OK);
    }

    #[test]
    fn count_prompt_tokens_estimates_from_body_len() {
        assert_eq!(count_prompt_tokens(0, 3), None);
        assert_eq!(count_prompt_tokens(99, 3), Some(33));
        assert_eq!(count_prompt_tokens(100, 3), Some(33));
    }

    #[test]
    fn count_prompt_tokens_divisor_scales_estimate() {
        assert_eq!(count_prompt_tokens(100, 1), Some(100));
        assert_eq!(count_prompt_tokens(100, 5), Some(20));
    }
}
