use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    Json,
    body::Body,
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use futures_util::StreamExt;
use reqwest::Request;
use tracing::{trace, warn};

use crate::app::AppState;
use crate::db::{ProxyEvent, SessionUser};
use crate::error::AitError;
use crate::providers::UpstreamProvider;

use super::guard::{ProxyLogGuard, UsageTokens, parse_usage};
use super::sse::SseTransformStream;

/// Collect `x-*` and `retry-after` headers from an upstream response.
fn collect_x_headers(headers: &HeaderMap) -> Vec<(HeaderName, HeaderValue)> {
    let mut filtered = Vec::new();
    for (name, value) in headers {
        let lower = name.as_str().to_ascii_lowercase();
        if lower.starts_with("x-") || lower == "retry-after" {
            filtered.push((name.clone(), value.clone()));
        }
    }
    filtered
}

/// Build a 502 error for upstream redirect responses.
fn redirect_error(status: StatusCode, provider_name: &str) -> (StatusCode, Json<AitError>) {
    warn!(
        "[proxy] upstream returned redirect {} for '{}'",
        status, provider_name
    );
    AitError::upstream_error(
        502,
        "Ait does not support redirect policy. If the provider's base_url \
         has changed, please update the provider configuration.",
    )
    .into_response()
}

pub(crate) async fn proxy_non_streamed(
    state: AppState,
    request: Request,
    upstream: Arc<dyn UpstreamProvider>,
    model_name: &str,
    provider: &crate::db::Provider,
    start: Instant,
) -> Result<(impl IntoResponse, UsageTokens, i64, usize), (StatusCode, Json<AitError>)> {
    trace!(
        model = model_name,
        "proxy_non_streamed: execute start, elapsed={}ms",
        start.elapsed().as_millis()
    );
    let response = state.http_client.execute(request).await.map_err(|e| {
        tracing::warn!("Failed to connect to provider '{}': {}", provider.name, e);
        AitError::upstream_error(502, "upstream request failed").into_response()
    })?;
    trace!(
        model = model_name,
        "proxy_non_streamed: response received, elapsed={}ms",
        start.elapsed().as_millis()
    );

    let ttfb = start.elapsed().as_millis() as i64;
    let status = response.status();

    if status.is_redirection() {
        return Err(redirect_error(status, &provider.name));
    }

    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("content-type"),
        "application/json".parse().unwrap(),
    );
    for (name, value) in collect_x_headers(response.headers()) {
        headers.insert(name, value);
    }

    trace!(
        model = model_name,
        "proxy_non_streamed: fetching body, elapsed={}ms",
        start.elapsed().as_millis()
    );
    let timeout = Duration::from_secs(state.config.proxy.timeout_secs);
    let bytes = tokio::time::timeout(timeout, response.bytes())
        .await
        .map_err(|_| AitError::upstream_error(408, "upstream read timeout").into_response())?
        .map_err(|e| {
            tracing::warn!("Upstream body read error: {}", e);
            AitError::upstream_error(502, "upstream request failed").into_response()
        })?;
    trace!(
        model = model_name,
        body_size = bytes.len(),
        "proxy_non_streamed: body fetched, elapsed={}ms",
        start.elapsed().as_millis()
    );

    if !status.is_success() {
        let body_str = String::from_utf8_lossy(&bytes);
        let truncated = if body_str.len() > 512 {
            let end = body_str.floor_char_boundary(512);
            format!("{}...", &body_str[..end])
        } else {
            body_str.to_string()
        };
        warn!("[proxy] upstream error {}: {}", status, truncated);
        return Err(AitError::upstream_error(status.as_u16(), truncated).into_response());
    }

    let body_size = bytes.len();
    let body = upstream.transform_response(&bytes, model_name);
    let usage = parse_usage(&body);
    Ok(((StatusCode::OK, headers, body), usage, ttfb, body_size))
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn proxy_streamed(
    state: AppState,
    request: Request,
    upstream: Arc<dyn UpstreamProvider>,
    provider_name: String,
    model_name: String,
    session: SessionUser,
    start: Instant,
    endpoint: String,
    upstream_model: String,
    provider_type: String,
    client_ip: Option<String>,
    prompt_tokens: Option<i64>,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    let log_manager = state.log_manager.clone();
    let model_name_for_transform = model_name.clone();
    let base_event = ProxyEvent {
        // Owned by SseTransformStream in the success path — it writes the log
        // on stream completion via Drop.  The guard below holds a clone for
        // error coverage (before the stream is set up); it is suppressed once
        // execution succeeds so only the stream writes the final record.
        timestamp: Utc::now(),
        username: Some(session.username),
        api_key_name: session.api_key_name,
        model_name,
        provider_name,
        prompt_tokens,
        completion_tokens: None,
        total_tokens: None,
        cached_tokens: None,
        latency_ms: 0,
        status: "200".to_string(),
        endpoint,
        is_streaming: true,
        time_to_first_token_ms: None,
        upstream_model,
        provider_type,
        response_body_size: None,
        error_message: None,
        client_ip,
    };

    // Clone: covers execute / redirect / non-2xx errors before the stream
    // exists.  On success suppress_drop_log prevents the guard's Drop from
    // writing a duplicate 499 — only SseTransformStream::Drop logs the event.
    let mut guard = ProxyLogGuard::new(log_manager.clone(), base_event.clone(), start);

    trace!(
        model = base_event.model_name,
        "proxy_streamed: execute start"
    );
    let response = state.http_client.execute(request).await.map_err(|e| {
        tracing::warn!("Failed to connect to provider: {}", e);
        guard.event.error_message = Some(e.to_string());
        guard.finalize(&UsageTokens::default(), "502");
        AitError::upstream_error(502, "upstream request failed").into_response()
    })?;
    trace!(
        model = base_event.model_name,
        "proxy_streamed: response received"
    );

    let status = response.status();

    if status.is_redirection() {
        guard.event.error_message = Some(format!("upstream returned redirect {}", status));
        guard.finalize(&UsageTokens::default(), "502");
        return Err(redirect_error(status, &base_event.provider_name));
    }

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        warn!("[proxy] upstream error {}: {}", status, body);
        let truncated = if body.len() > 512 {
            let end = body.floor_char_boundary(512);
            format!("{}...", &body[..end])
        } else {
            body
        };
        guard.event.error_message = Some(truncated);
        guard.finalize(&UsageTokens::default(), &status.as_u16().to_string());
        return Err(AitError::upstream_error(
            status.as_u16(),
            "upstream request failed".to_string(),
        )
        .into_response());
    }

    // Success path: suppress the guard so only SseTransformStream::Drop
    // writes the log record.  Without this the guard's Drop would emit a 499
    // while the stream still holds the original (this clone) and logs it too.
    guard.suppress_drop_log();

    let mut stream_builder = Response::builder()
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache");

    for (name, value) in collect_x_headers(response.headers()) {
        stream_builder = stream_builder.header(name, value);
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
        user_tokens: None,
        log_manager,
        event: base_event,
        start,
        done: false,
        shutdown_fut: Box::pin(state.shutdown_token.clone().cancelled_owned()),
        idle_timeout: Duration::from_secs(state.config.proxy.sse_idle_timeout_secs),
        last_data_time: Instant::now(),
    };

    let body = Body::from_stream(sse_stream);
    let resp = stream_builder
        .body(body)
        .expect("static SSE headers are valid");
    Ok(resp)
}
