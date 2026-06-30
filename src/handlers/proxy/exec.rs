use std::sync::Arc;
use std::time::Instant;

use axum::{
    Json,
    body::Body,
    http::{HeaderMap, HeaderName, StatusCode},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use futures_util::StreamExt;
use reqwest::Request;
use tracing::warn;

use crate::app::AppState;
use crate::db::{ProxyEvent, SessionUser};
use crate::error::AitError;
use crate::providers::UpstreamProvider;

use super::guard::{ProxyLogGuard, UsageTokens, parse_usage};
use super::sse::SseTransformStream;

pub(crate) async fn proxy_non_streamed(
    state: AppState,
    request: Request,
    upstream: Arc<dyn UpstreamProvider>,
    model_name: &str,
    provider: &crate::db::Provider,
) -> Result<(impl IntoResponse, UsageTokens), (StatusCode, Json<AitError>)> {
    let response = state.http_client.execute(request).await.map_err(|e| {
        AitError::upstream_error(
            502,
            format!("Failed to connect to provider '{}': {}", provider.name, e),
        )
        .into_response()
    })?;

    let status = response.status();

    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("content-type"),
        "application/json".parse().unwrap(),
    );
    for (name, value) in response.headers() {
        let lower = name.as_str().to_ascii_lowercase();
        if lower.starts_with("x-") || lower == "retry-after" {
            headers.insert(name.clone(), value.clone());
        }
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| AitError::upstream_error(502, e.to_string()).into_response())?;

    if !status.is_success() {
        warn!(
            "[proxy] upstream error {}: {}",
            status,
            String::from_utf8_lossy(&bytes)
        );
        return Err(AitError::upstream_error(
            status.as_u16(),
            "upstream request failed".to_string(),
        )
        .into_response());
    }

    let body = upstream.transform_response(&bytes, model_name);
    let usage = parse_usage(&body);
    Ok(((StatusCode::OK, headers, body), usage))
}

pub(crate) async fn proxy_streamed(
    state: AppState,
    request: Request,
    upstream: Arc<dyn UpstreamProvider>,
    provider_name: String,
    model_name: String,
    session: SessionUser,
    start: Instant,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    let log_manager = state.log_manager.clone();
    let model_name_for_transform = model_name.clone();
    let base_event = ProxyEvent {
        timestamp: Utc::now(),
        username: Some(session.username),
        api_key_name: session.api_key_name,
        model_name,
        provider_name,
        prompt_tokens: None,
        completion_tokens: None,
        total_tokens: None,
        cached_tokens: None,
        latency_ms: 0,
        status: "200".to_string(),
    };

    let mut guard = ProxyLogGuard::new(log_manager.clone(), base_event.clone(), start);

    let response = state.http_client.execute(request).await.map_err(|e| {
        guard.finalize(&UsageTokens::default(), "502");
        AitError::upstream_error(502, format!("Failed to connect to provider: {}", e))
            .into_response()
    })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        warn!("[proxy] upstream error {}: {}", status, body);
        guard.finalize(&UsageTokens::default(), &status.as_u16().to_string());
        return Err(AitError::upstream_error(
            status.as_u16(),
            "upstream request failed".to_string(),
        )
        .into_response());
    }

    guard.suppress_drop_log();

    let mut stream_builder = Response::builder()
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache");

    for (name, value) in response.headers() {
        let lower = name.as_str().to_ascii_lowercase();
        if lower.starts_with("x-") || lower == "retry-after" {
            stream_builder = stream_builder.header(name.clone(), value.clone());
        }
    }

    let raw_stream = response.bytes_stream().map(|result| {
        result.map_err(|e| {
            warn!("Stream error: {}", e);
            std::io::Error::other(e)
        })
    });

    let sse_stream = SseTransformStream {
        inner: raw_stream,
        buf: bytes::BytesMut::new(),
        upstream,
        model_name: model_name_for_transform,
        last_payload: None,
        user_tokens: None,
        log_manager,
        event: base_event,
        start,
        done: false,
        shutdown_fut: Box::pin(state.shutdown_token.clone().cancelled_owned()),
    };

    let body = Body::from_stream(sse_stream);
    let resp = stream_builder
        .body(body)
        .expect("static SSE headers are valid");
    Ok(resp)
}
