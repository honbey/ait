use axum::{
    extract::{State, Json},
    http::{HeaderMap, HeaderName, StatusCode},
    response::{sse::Event, IntoResponse, Response, Sse},
    Json as AxumJson,
};
use chrono::Utc;
use futures_util::StreamExt;
use tracing::{info, warn};

use crate::app::AppState;
use crate::providers::{OpenAIError, create_provider};

pub async fn chat_completions(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Response, (StatusCode, Json<OpenAIError>)> {
    proxy_request(state, body, "/v1/chat/completions").await
}

pub async fn completions(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Response, (StatusCode, Json<OpenAIError>)> {
    proxy_request(state, body, "/v1/completions").await
}

pub async fn embeddings(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Response, (StatusCode, Json<OpenAIError>)> {
    proxy_request(state, body, "/v1/embeddings").await
}

pub async fn health(State(state): State<AppState>) -> AxumJson<serde_json::Value> {
    if state.config.server.health_detail {
        let uptime = Utc::now() - state.start_time;
        let total_secs = uptime.num_seconds().max(0) as u64;
        let hours = total_secs / 3600;
        let mins = (total_secs % 3600) / 60;
        let secs = total_secs % 60;

        let providers_count = state.db.list_providers().unwrap_or_default().len();
        let models_count = state.db.list_models().unwrap_or_default().len();

        AxumJson(serde_json::json!({
            "status": "ok",
            "version": env!("CARGO_PKG_VERSION"),
            "uptime": format!("{}h{}m{}s", hours, mins, secs),
            "uptime_secs": total_secs,
            "start_time": state.start_time.to_rfc3339(),
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
) -> Result<Json<serde_json::Value>, (StatusCode, Json<OpenAIError>)> {
    let models = state.db.list_models().map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(OpenAIError::internal_error(e)))
    })?;

    let providers = state.db.list_providers().map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(OpenAIError::internal_error(e)))
    })?;

    let data: Vec<serde_json::Value> = models
        .into_iter()
        .filter(|m| m.enabled)
        .map(|m| {
            let owned_by = providers
                .iter()
                .find(|p| p.id == m.provider_id)
                .map(|p| p.name.as_str())
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

// --- Core Proxy Logic ---

pub async fn proxy_request(
    state: AppState,
    body: serde_json::Value,
    upstream_path: &str,
) -> Result<Response, (StatusCode, Json<OpenAIError>)> {
    // Extract model name
    let model_name = body
        .get("model")
        .and_then(|m| m.as_str())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(OpenAIError::bad_request("Missing 'model' field in request body")),
            )
        })?;

    info!("Routing request for model: {}", model_name);

    // Resolve model -> provider
    let (model, provider) = match state.db.resolve_model(model_name) {
        Ok(Some((m, p))) => (m, p),
        Ok(None) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(OpenAIError::not_found(format!(
                    "Model '{}' not found or disabled",
                    model_name
                ))),
            ));
        }
        Err(e) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(OpenAIError::internal_error(e)),
            ));
        }
    };

    // Check if stream is requested
    let stream = body
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false)
        && state.config.proxy.stream;

    // Build upstream request
    let upstream = create_provider(&provider, state.http_client.clone());
    let request = upstream
        .build_request(&state.http_client, &body, stream, &model.upstream_model, upstream_path)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(OpenAIError::bad_request(e)),
            )
        })?;

    info!(
        "Proxying to provider '{}' for model '{}' -> upstream '{}', base_url: {}",
        provider.name, model.name, model.upstream_model, request.url()
    );

    if stream {
        let resp = proxy_streamed(state, request, &provider, &model).await?;
        Ok(resp.into_response())
    } else {
        let resp = proxy_non_streamed(state, request, &provider, &model).await?;
        Ok(resp.into_response())
    }
}

async fn proxy_non_streamed(
    state: AppState,
    request: reqwest::Request,
    provider: &crate::db::Provider,
    _model: &crate::db::Model,
) -> Result<impl IntoResponse + use<>, (StatusCode, Json<OpenAIError>)> {
    let response = state
        .http_client
        .execute(request)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(OpenAIError::upstream_error(
                    502,
                    format!("Failed to connect to provider '{}': {}", provider.name, e),
                )),
            )
        })?;

    let status = response.status();
    let bytes = response.bytes().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(OpenAIError::upstream_error(502, e.to_string())),
        )
    })?;

    if !status.is_success() {
        return Err((
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(OpenAIError::upstream_error(
                status.as_u16(),
                String::from_utf8_lossy(&bytes).to_string(),
            )),
        ));
    }

    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("content-type"),
        "application/json".parse().unwrap(),
    );
    Ok((StatusCode::OK, headers, bytes.to_vec()))
}

async fn proxy_streamed(
    state: AppState,
    request: reqwest::Request,
    provider: &crate::db::Provider,
    _model: &crate::db::Model,
) -> Result<impl IntoResponse + use<>, (StatusCode, Json<OpenAIError>)> {
    let response = state
        .http_client
        .execute(request)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(OpenAIError::upstream_error(
                    502,
                    format!("Failed to connect to provider '{}': {}", provider.name, e),
                )),
            )
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err((
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(OpenAIError::upstream_error(status.as_u16(), body)),
        ));
    }

    let stream = Sse::new(
        response
            .bytes_stream()
            .map(|result| match result {
                Ok(bytes) => Ok(Event::default().data(String::from_utf8_lossy(&bytes))),
                Err(e) => {
                    warn!("Stream error: {}", e);
                    Ok::<Event, std::convert::Infallible>(
                        Event::default().event("error").data(e.to_string()),
                    )
                }
            }),
    );

    Ok(stream)
}