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

use crate::app::{AppState, ModelCacheEntry, NEGATIVE_CACHE_TTL};
use crate::db::{ApiKeyContext, Model, Provider, ProxyEvent, RequestId};
use crate::error::{AitError, internal_error, not_found};
use crate::middleware::CACHE_TTL;
use crate::providers::create_provider;
use crate::ssrf;
use crate::utils::mask_sensitive_value;

/// Insert into the model cache only while under the entry cap; beyond it the
/// entry is not cached at all. `DashMap::len` is approximate under concurrency,
/// so the map stays bounded within a small multiple of `cache_max_entries`.
fn insert_model_cache(
    map: &dashmap::DashMap<String, ModelCacheEntry>,
    name: &str,
    entry: ModelCacheEntry,
    max_entries: usize,
) {
    if map.len() < max_entries {
        map.insert(name.to_string(), entry);
    }
}

mod exec;
mod guard;
mod sse;

use exec::{proxy_non_streamed, proxy_streamed};
use guard::{ProxyLogGuard, UsageTokens};

pub async fn chat_completions(
    State(state): State<AppState>,
    Extension(api_key): Extension<ApiKeyContext>,
    Extension(client_ip): Extension<Option<IpAddr>>,
    Extension(request_id): Extension<RequestId>,
    body: bytes::Bytes,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    let body_len = body.len();
    let body = serde_json::from_slice(&body)
        .map_err(|_| AitError::bad_request("invalid request body").into_response())?;
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
    let body_len = body.len();
    let body = serde_json::from_slice(&body)
        .map_err(|_| AitError::bad_request("invalid request body").into_response())?;
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
    let body_len = body.len();
    let body = serde_json::from_slice(&body)
        .map_err(|_| AitError::bad_request("invalid request body").into_response())?;
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
    let body_len = body.len();
    let body = serde_json::from_slice(&body)
        .map_err(|_| AitError::bad_request("invalid request body").into_response())?;
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

    if body_len as u64 > state.config.proxy.max_request_body_bytes {
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
                    state
                        .provider_cache
                        .insert(p.id.clone(), (upstream, Instant::now()));
                    insert_model_cache(
                        &state.model_cache,
                        model_name,
                        (Some((m.clone(), p.clone())), Instant::now()),
                        max_entries,
                    );
                    Ok((m, p))
                }
                Ok(Ok(None)) => {
                    insert_model_cache(
                        &state.model_cache,
                        model_name,
                        (None, Instant::now()),
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

        let cached = state.model_cache.get_mut(model_name).and_then(|mut entry| {
            let ttl = if entry.0.is_none() {
                NEGATIVE_CACHE_TTL
            } else {
                CACHE_TTL
            };
            if entry.1.elapsed() < ttl {
                // slide only positive entries; negative ones expire for real
                if entry.0.is_some() {
                    entry.1 = Instant::now();
                }
                Some(entry.0.clone())
            } else {
                None
            }
        });
        match cached {
            Some(Some((m, p))) => (m, p),
            Some(None) => {
                return Err(not_found(format!(
                    "Model '{}' not found or disabled",
                    model_name
                )));
            }
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
            state
                .provider_cache
                .insert(provider.id.clone(), (upstream.clone(), Instant::now()));
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

    let prompt_tokens = count_prompt_tokens(body_len);

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

    ssrf::check_ssrf(
        request.url(),
        &state.config.security.ssrf_allowed_cidrs,
        &state.ssrf_dns_cache,
        &provider_name,
    )
    .await?;

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
fn count_prompt_tokens(body_len: usize) -> Option<i64> {
    if body_len == 0 {
        None
    } else {
        Some(body_len as i64 / 3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{ApiKeyContext, RequestId};
    use crate::test_utils::create_test_state_dlp;
    use axum::Extension;
    use bytes::Bytes;

    #[test]
    fn insert_model_cache_respects_cap() {
        let map = dashmap::DashMap::new();
        let entry = (None, Instant::now());
        for i in 0..5 {
            insert_model_cache(&map, &format!("m{i}"), entry.clone(), 5);
        }
        assert_eq!(map.len(), 5);
        insert_model_cache(&map, "m6", entry.clone(), 5);
        assert_eq!(map.len(), 5);
    }

    #[test]
    fn insert_model_cache_zero_cap_caches_nothing() {
        let map = dashmap::DashMap::new();
        insert_model_cache(&map, "m0", (None, Instant::now()), 0);
        assert!(map.is_empty());
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
}
