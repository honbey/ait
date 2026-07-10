use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;

use futures_util::Stream;
use tokio_util::sync::WaitForCancellationFutureOwned;

use crate::db::{LogManager, ProxyEvent};
use crate::providers::UpstreamProvider;

use super::guard::{UsageTokens, parse_usage};

pub(crate) struct SseTransformStream<S> {
    pub(crate) inner: S,
    pub(crate) buf: bytes::BytesMut,
    pub(crate) upstream: Arc<dyn UpstreamProvider>,
    pub(crate) model_name: String,
    pub(crate) user_tokens: Option<UsageTokens>,
    pub(crate) log_manager: LogManager,
    pub(crate) event: ProxyEvent,
    pub(crate) start: Instant,
    pub(crate) shutdown_fut: Pin<Box<WaitForCancellationFutureOwned>>,
    pub(crate) done: bool,
}

impl<S> SseTransformStream<S> {
    fn find_event_boundary(&self) -> Option<usize> {
        if let Some(i) = self.buf.windows(4).position(|w| w == b"\r\n\r\n") {
            return Some(i + 4);
        }
        self.buf
            .windows(2)
            .position(|w| w == b"\n\n")
            .map(|i| i + 2)
    }

    fn try_extract_usage_from(&mut self, payload: &[u8]) {
        let tokens = parse_usage(payload);
        if tokens.prompt_tokens.is_some_and(|v| v > 0)
            || tokens.completion_tokens.is_some_and(|v| v > 0)
            || tokens.total_tokens.is_some_and(|v| v > 0)
        {
            self.user_tokens = Some(tokens);
        }
    }

    fn record_ttfb(&mut self) {
        if self.event.time_to_first_token_ms.is_none() {
            self.event.time_to_first_token_ms = Some(self.start.elapsed().as_millis() as i64);
        }
    }

    fn finalize_log(&mut self) {
        if let Some(usage) = self.user_tokens.take() {
            self.event.prompt_tokens = usage.prompt_tokens;
            self.event.completion_tokens = usage.completion_tokens;
            self.event.total_tokens = usage.total_tokens;
            self.event.cached_tokens = usage.cached_tokens;
        }
        if self.event.time_to_first_token_ms.is_none() {
            self.event.time_to_first_token_ms = Some(self.start.elapsed().as_millis() as i64);
        }
        self.event.latency_ms = self.start.elapsed().as_millis() as i64;
        self.log_manager.log_proxy(self.event.clone());
    }

    fn finalize_stream(&mut self) -> Poll<Option<Result<bytes::Bytes, std::io::Error>>> {
        self.done = true;
        self.finalize_log();

        if !self.buf.is_empty() {
            let remaining = std::mem::take(&mut self.buf);
            let transformed = self
                .upstream
                .transform_response(&remaining, &self.model_name);
            return Poll::Ready(Some(Ok(bytes::Bytes::from(transformed))));
        }
        Poll::Ready(None)
    }

    fn transform_event(&mut self, event: &[u8]) -> Vec<u8> {
        let Ok(text) = std::str::from_utf8(event) else {
            return event.to_vec();
        };

        let mut out = String::with_capacity(event.len() + 64);
        for line in text.lines() {
            if let Some(payload) = line.strip_prefix("data: ") {
                self.try_extract_usage_from(payload.as_bytes());
                let transformed = self
                    .upstream
                    .transform_response(payload.as_bytes(), &self.model_name);
                out.push_str("data: ");
                if let Ok(t) = std::str::from_utf8(&transformed) {
                    out.push_str(t);
                } else {
                    out.push_str(payload);
                }
                out.push('\n');
            } else if !line.is_empty() || !out.ends_with("\n\n") {
                out.push_str(line);
                out.push('\n');
            }
        }
        out.into_bytes()
    }
}

impl<S> Drop for SseTransformStream<S> {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        self.event.status = "499".to_string();
        self.finalize_log();
    }
}

impl<S> Stream for SseTransformStream<S>
where
    S: Stream<Item = Result<bytes::Bytes, std::io::Error>> + Unpin,
{
    type Item = Result<bytes::Bytes, std::io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        if this.done {
            return Poll::Ready(None);
        }

        if let Some(event_end) = this.find_event_boundary() {
            this.record_ttfb();
            let event = this.buf.split_to(event_end);
            let transformed = this.transform_event(&event);
            this.event.response_body_size =
                Some(this.event.response_body_size.unwrap_or(0) + transformed.len() as i64);
            return Poll::Ready(Some(Ok(bytes::Bytes::from(transformed))));
        }

        if Pin::new(&mut this.shutdown_fut).poll(cx).is_ready() {
            return this.finalize_stream();
        }

        loop {
            match Pin::new(&mut this.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    this.buf.extend_from_slice(&bytes);
                    if let Some(event_end) = this.find_event_boundary() {
                        this.record_ttfb();
                        let event = this.buf.split_to(event_end);
                        let transformed = this.transform_event(&event);
                        this.event.response_body_size = Some(
                            this.event.response_body_size.unwrap_or(0) + transformed.len() as i64,
                        );
                        return Poll::Ready(Some(Ok(bytes::Bytes::from(transformed))));
                    }
                }
                Poll::Ready(Some(Err(e))) => {
                    this.event.error_message = Some(e.to_string());
                    this.finalize_log();
                    this.done = true;
                    return Poll::Ready(Some(Err(e)));
                }
                Poll::Ready(None) => return this.finalize_stream(),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
