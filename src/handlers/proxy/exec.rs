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
use tokio::time::sleep;
use tracing::{trace, warn};

use crate::app::AppState;
use crate::db::{ApiKeyContext, ProxyEvent, RequestId};
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
    client: reqwest::Client,
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
    let response = client.execute(request).await.map_err(|e| {
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
    let max_body = state.config.proxy.max_response_body_bytes as usize;
    let mut body = Vec::new();
    tokio::time::timeout(timeout, async {
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| {
                tracing::warn!("Upstream body read error: {}", e);
            })?;
            if body.len() + chunk.len() > max_body {
                return Err(());
            }
            body.extend_from_slice(&chunk);
        }
        Ok::<_, ()>(())
    })
    .await
    .map_err(|_| AitError::upstream_error(408, "upstream read timeout").into_response())?
    .map_err(|_| AitError::upstream_error(502, "upstream response too large").into_response())?;
    let bytes = body;
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
        // Keep the upstream error body out of the client response; it may
        // contain internal paths or stack traces.  Only the status message
        // reaches the client, the body is recorded in the request log.
        return Err(
            AitError::upstream_error(status.as_u16(), "upstream request failed")
                .with_detail(truncated)
                .into_response(),
        );
    }

    let body_size = bytes.len();
    let body = upstream.transform_response(&bytes, model_name);
    let usage = parse_usage(&body);
    Ok(((StatusCode::OK, headers, body), usage, ttfb, body_size))
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn proxy_streamed(
    state: AppState,
    client: reqwest::Client,
    request: Request,
    upstream: Arc<dyn UpstreamProvider>,
    provider_name: String,
    model_name: String,
    api_key: ApiKeyContext,
    start: Instant,
    endpoint: String,
    upstream_model: String,
    provider_type: String,
    client_ip: Option<String>,
    prompt_tokens: Option<i64>,
    request_id: RequestId,
) -> Result<Response, (StatusCode, Json<AitError>)> {
    let log_manager = state.log_manager.clone();
    let model_name_for_transform = model_name.clone();
    let base_event = ProxyEvent {
        // Owned by SseTransformStream in the success path — it writes the log
        // on stream completion via Drop.  The guard below holds a clone for
        // error coverage (before the stream is set up); it is suppressed once
        // execution succeeds so only the stream writes the final record.
        timestamp: Utc::now(),
        request_id: request_id.0,
        api_key_name: api_key.name.clone(),
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
    let response = client.execute(request).await.map_err(|e| {
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
        let timeout = Duration::from_secs(state.config.proxy.timeout_secs);
        let body = tokio::time::timeout(timeout, response.text())
            .await
            .map_err(|_| AitError::upstream_error(408, "upstream read timeout").into_response())?
            .unwrap_or_default();
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

    let idle_timeout = Duration::from_secs(state.config.proxy.sse_idle_timeout_secs);
    // Zero disables the cap for the two new guards; `sse_idle_timeout_secs`
    // keeps its literal meaning so existing configs behave unchanged.
    let max_duration = match state.config.proxy.sse_max_duration_secs {
        0 => Duration::MAX,
        secs => Duration::from_secs(secs),
    };
    let max_bytes = match state.config.proxy.max_response_body_bytes {
        0 => usize::MAX,
        bytes => bytes as usize,
    };

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
        idle_timeout,
        idle_timer: Box::pin(sleep(idle_timeout)),
        max_duration,
        total_bytes: 0,
        max_bytes,
    };

    let body = Body::from_stream(sse_stream);
    let resp = stream_builder
        .body(body)
        .expect("static SSE headers are valid");
    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, StatusCode};

    #[test]
    fn collect_x_headers_filters_x_and_retry_after() {
        let mut headers = HeaderMap::new();
        headers.insert("x-request-id", "abc123".parse().unwrap());
        headers.insert("x-ratelimit-remaining", "100".parse().unwrap());
        headers.insert("retry-after", "60".parse().unwrap());
        headers.insert("content-type", "application/json".parse().unwrap());
        headers.insert("authorization", "Bearer sk-x".parse().unwrap());

        let collected = collect_x_headers(&headers);
        assert_eq!(collected.len(), 3);
        let names: Vec<String> = collected
            .iter()
            .map(|(n, _)| n.as_str().to_string())
            .collect();
        assert!(names.contains(&"x-request-id".to_string()));
        assert!(names.contains(&"x-ratelimit-remaining".to_string()));
        assert!(names.contains(&"retry-after".to_string()));
    }

    #[test]
    fn collect_x_headers_empty_when_no_matches() {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());
        headers.insert("content-length", "42".parse().unwrap());
        let collected = collect_x_headers(&headers);
        assert!(collected.is_empty());
    }

    #[test]
    fn redirect_error_returns_502() {
        let (status, json) = redirect_error(StatusCode::MOVED_PERMANENTLY, "test-provider");
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(json.0.code, 502);
        assert!(json.0.message.contains("redirect"));
    }
}
