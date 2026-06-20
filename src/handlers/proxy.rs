use axum::{
    Extension, Json as AxumJson,
    extract::{Json, State},
    http::{HeaderMap, HeaderName, StatusCode},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use futures_util::StreamExt;
use tracing::{info, warn};

use crate::app::AppState;
use crate::db::{Permission, SessionUser, UserRole};
use crate::error::AitError;
use crate::providers::create_provider;

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

        let providers_count = state.db.list_providers().unwrap_or_default().len();
        let models_count = state.db.list_models().unwrap_or_default().len();

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
    Extension(session): Extension<SessionUser>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AitError>)> {
    let models = state.db.list_models()?;

    let providers = state.db.list_providers()?;

    let data: Vec<serde_json::Value> = models
        .into_iter()
        .filter(|m| m.enabled)
        .filter(|m| match session.role {
            UserRole::Admin => true,
            UserRole::User => session.allowed.iter().any(|a| {
                a.provider_id == m.provider_id
                    && (a.model_names.is_empty() || a.model_names.contains(&m.name))
            }),
        })
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

fn check_model_access(
    model_provider_id: &str,
    model_name: &str,
    session: &SessionUser,
) -> Result<(), (StatusCode, Json<AitError>)> {
    match session.role {
        UserRole::Admin => Ok(()),
        UserRole::User => {
            let has_access = session.allowed.iter().any(|a: &Permission| {
                a.provider_id == model_provider_id
                    && (a.model_names.is_empty() || a.model_names.iter().any(|n| n == model_name))
            });
            if has_access {
                Ok(())
            } else {
                Err((
                    StatusCode::FORBIDDEN,
                    Json(AitError {
                        message: format!("You don't have access to model '{}'", model_name),
                        code: 403,
                        r#type: "forbidden".to_string(),
                    }),
                ))
            }
        }
    }
}

// --- Core Proxy Logic ---

pub async fn proxy_request(
    state: AppState,
    session: SessionUser,
    body: serde_json::Value,
    upstream_path: &str,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    // Extract model name
    let model_name = body.get("model").and_then(|m| m.as_str()).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(AitError::bad_request(
                "Missing 'model' field in request body",
            )),
        )
    })?;

    info!("Routing request for model: {}", model_name);

    // Resolve model -> provider
    let (model, provider) = match state.db.resolve_model(model_name) {
        Ok(Some((m, p))) => (m, p),
        Ok(None) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(AitError::not_found(format!(
                    "Model '{}' not found or disabled",
                    model_name
                ))),
            ));
        }
        Err(e) => {
            return Err(AitError::from_db_error(e));
        }
    };

    // Check access permissions
    check_model_access(provider.id.as_str(), model_name, &session)?;

    // Check if stream is requested
    let stream = body
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false)
        && state.config.proxy.stream;

    // Build upstream request
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
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(AitError::bad_request(e))))?;

    info!(
        "Proxying to provider '{}' for model '{}' -> upstream '{}', base_url: {}",
        provider.name,
        model.name,
        model.upstream_model,
        request.url()
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
) -> Result<impl IntoResponse + use<>, (StatusCode, Json<AitError>)> {
    let response = state.http_client.execute(request).await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(AitError::upstream_error(
                502,
                format!("Failed to connect to provider '{}': {}", provider.name, e),
            )),
        )
    })?;

    let status = response.status();
    let bytes = response.bytes().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(AitError::upstream_error(502, e.to_string())),
        )
    })?;

    if !status.is_success() {
        return Err((
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(AitError::upstream_error(
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
) -> Result<impl IntoResponse + use<>, (StatusCode, Json<AitError>)> {
    let response = state.http_client.execute(request).await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(AitError::upstream_error(
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
            Json(AitError::upstream_error(status.as_u16(), body)),
        ));
    }

    let mut stream_builder = axum::response::Response::builder()
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache");

    for (name, value) in response.headers() {
        if name.as_str().to_ascii_lowercase().starts_with("x-") {
            stream_builder = stream_builder.header(name.clone(), value.clone());
        }
    }

    let stream = response.bytes_stream().map(|result| {
        result.map_err(|e| {
            warn!("Stream error: {}", e);
            std::io::Error::other(e)
        })
    });

    let body = axum::body::Body::from_stream(stream);

    let resp = stream_builder
        .body(body)
        .expect("static SSE headers are valid");

    Ok(resp)
}
