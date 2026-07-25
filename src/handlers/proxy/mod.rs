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

use crate::app::AppState;
use crate::db::{Model, Provider, ProxyEvent, RequestId, SessionUser};
use crate::error::{AitError, internal_error, not_found};
use crate::middleware::CACHE_TTL;
use crate::providers::create_provider;
use crate::ssrf;

mod exec;
mod guard;
mod sse;

use exec::{proxy_non_streamed, proxy_streamed};
use guard::{ProxyLogGuard, UsageTokens};

pub async fn chat_completions(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Extension(client_ip): Extension<Option<IpAddr>>,
    Extension(request_id): Extension<RequestId>,
    body: bytes::Bytes,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    let body_len = body.len();
    let body = serde_json::from_slice(&body)
        .map_err(|_| AitError::bad_request("invalid request body").into_response())?;
    proxy_request(
        state,
        session,
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
    Extension(session): Extension<SessionUser>,
    Extension(client_ip): Extension<Option<IpAddr>>,
    Extension(request_id): Extension<RequestId>,
    body: bytes::Bytes,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    let body_len = body.len();
    let body = serde_json::from_slice(&body)
        .map_err(|_| AitError::bad_request("invalid request body").into_response())?;
    proxy_request(
        state,
        session,
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
    Extension(session): Extension<SessionUser>,
    Extension(client_ip): Extension<Option<IpAddr>>,
    Extension(request_id): Extension<RequestId>,
    body: bytes::Bytes,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    let body_len = body.len();
    let body = serde_json::from_slice(&body)
        .map_err(|_| AitError::bad_request("invalid request body").into_response())?;
    proxy_request(
        state,
        session,
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
    Extension(session): Extension<SessionUser>,
    Extension(client_ip): Extension<Option<IpAddr>>,
    Extension(request_id): Extension<RequestId>,
    body: bytes::Bytes,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    let body_len = body.len();
    let body = serde_json::from_slice(&body)
        .map_err(|_| AitError::bad_request("invalid request body").into_response())?;
    proxy_request(
        state,
        session,
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

        let db = state.db.clone();
        let (providers_count, models_count) = crate::run_blocking(move || {
            let p = db.count_providers().unwrap_or(0);
            let m = db.count_models().unwrap_or(0);
            (p, m)
        })
        .await
        .unwrap_or((0, 0));

        AxumJson(serde_json::json!({
            "status": "ok",
            "version": env!("CARGO_PKG_VERSION"),
            "uptime": format!("{}h{}m{}s", hours, mins, secs),
            "uptime_secs": total_secs,
            "start_time": state.start_time.timestamp(),
            "providers_count": providers_count,
            "models_count": models_count,
        }))
    } else {
        AxumJson(serde_json::json!({
            "status": "ok"
        }))
    }
}

pub async fn list_models_proxy(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionUser>,
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
    session: SessionUser,
    client_ip: Option<IpAddr>,
    request_id: RequestId,
    body: serde_json::Value,
    body_len: usize,
    upstream_path: &str,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    let start = Instant::now();

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
            match crate::run_blocking(move || db.resolve_model(&name)).await {
                Ok(Ok(Some((m, p)))) => {
                    let upstream = create_provider(&p, state.http_client.clone());
                    state
                        .provider_cache
                        .insert(p.id.clone(), (upstream, Instant::now()));
                    state.model_cache.insert(
                        model_name.to_string(),
                        (Some((m.clone(), p.clone())), Instant::now()),
                    );
                    Ok((m, p))
                }
                Ok(Ok(None)) => {
                    state
                        .model_cache
                        .insert(model_name.to_string(), (None, Instant::now()));
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
            if entry.1.elapsed() < CACHE_TTL {
                entry.1 = Instant::now();
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
            session.clone(),
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
            username: Some(session.username),
            api_key_name: session.api_key_name,
            model_name: model_name.clone(),
            provider_name: provider_name.clone(),
            prompt_tokens,
            completion_tokens: None,
            total_tokens: None,
            cached_tokens: None,
            latency_ms: 0,
            status: "200".to_string(),
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
            guard.event.error_message = Some(e.1.0.message.clone());
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
