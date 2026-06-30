use std::collections::HashMap;
use std::time::Instant;

use axum::{
    Extension, Json as AxumJson,
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use tracing::debug;

use crate::app::AppState;
use crate::db::{ProxyEvent, SessionUser};
use crate::error::{AitError, not_found};
use crate::providers::create_provider;

mod exec;
mod guard;
mod sse;

use exec::{proxy_non_streamed, proxy_streamed};
use guard::{ProxyLogGuard, UsageTokens};

pub async fn chat_completions(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Json(body): Json<serde_json::Value>,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    proxy_request(state, session, body, "/v1/chat/completions").await
}

pub async fn completions(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Json(body): Json<serde_json::Value>,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    proxy_request(state, session, body, "/v1/completions").await
}

pub async fn embeddings(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Json(body): Json<serde_json::Value>,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    proxy_request(state, session, body, "/v1/embeddings").await
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
        .await;

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
    .await?;

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
    body: serde_json::Value,
    upstream_path: &str,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    let start = Instant::now();

    let model_name = body.get("model").and_then(|m| m.as_str()).ok_or_else(|| {
        AitError::bad_request("Missing 'model' field in request body").into_response()
    })?;

    // Two sequential RocksDB get_cf (~20–100 µs); spawn_blocking overhead
    // (~5–20 µs) still exceeds the benefit, so called directly.
    let (model, provider) = match state.db.resolve_model(model_name) {
        Ok(Some((m, p))) => (m, p),
        Ok(None) => {
            return Err(not_found(format!(
                "Model '{}' not found or disabled",
                model_name
            )));
        }
        Err(e) => {
            return Err(AitError::from_db_error(e).into_response());
        }
    };

    let model_name = model.name.clone();
    let provider_name = provider.name.clone();

    let stream = body
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false)
        && state.config.proxy.stream;

    let upstream = create_provider(&provider, state.http_client.clone());
    let request = upstream
        .build_request(
            &state.http_client,
            &body,
            stream,
            &model.upstream_model,
            upstream_path,
        )
        .await
        .map_err(|e| AitError::bad_request(e).into_response())?;

    debug!(
        "Proxying to provider '{}' for model '{}' -> upstream '{}', base_url: {}",
        provider_name,
        model_name,
        model.upstream_model,
        request.url()
    );

    if stream {
        return proxy_streamed(
            state,
            request,
            upstream,
            provider_name,
            model_name,
            session.clone(),
            start,
        )
        .await
        .map(|r| r.into_response());
    }

    let log_manager = state.log_manager.clone();
    let mut guard = ProxyLogGuard::new(
        log_manager,
        ProxyEvent {
            timestamp: Utc::now(),
            username: Some(session.username),
            api_key_name: session.api_key_name,
            model_name: model_name.clone(),
            provider_name: provider_name.clone(),
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            cached_tokens: None,
            latency_ms: 0,
            status: "200".to_string(),
        },
        start,
    );

    match proxy_non_streamed(state.clone(), request, upstream, &model_name, &provider).await {
        Ok((resp, usage)) => {
            guard.finalize(&usage, "200");
            Ok(resp.into_response())
        }
        Err(e) => {
            guard.finalize(&UsageTokens::default(), &e.0.as_u16().to_string());
            Err(e)
        }
    }
}
