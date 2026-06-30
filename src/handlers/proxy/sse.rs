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
    pub(crate) last_payload: Option<Vec<u8>>,
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

    fn try_extract_usage(&mut self) {
        if self.user_tokens.is_some() {
            return;
        }
        let Some(last) = self.last_payload.take() else {
            return;
        };
        let tokens = parse_usage(&last);
        if tokens.prompt_tokens.is_some()
            || tokens.completion_tokens.is_some()
            || tokens.total_tokens.is_some()
        {
            self.user_tokens = Some(tokens);
        }
    }

    fn finalize_log(&mut self) {
        self.try_extract_usage();
        if let Some(usage) = self.user_tokens.take() {
            self.event.prompt_tokens = usage.prompt_tokens;
            self.event.completion_tokens = usage.completion_tokens;
            self.event.total_tokens = usage.total_tokens;
            self.event.cached_tokens = usage.cached_tokens;
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
                let transformed = self
                    .upstream
                    .transform_response(payload.as_bytes(), &self.model_name);
                if payload == "[DONE]" {
                    self.try_extract_usage();
                } else {
                    self.last_payload = Some(payload.as_bytes().to_vec());
                }
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
            let event = this.buf.split_to(event_end);
            let transformed = this.transform_event(&event);
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
                        let event = this.buf.split_to(event_end);
                        let transformed = this.transform_event(&event);
                        return Poll::Ready(Some(Ok(bytes::Bytes::from(transformed))));
                    }
                }
                Poll::Ready(Some(Err(e))) => return Poll::Ready(Some(Err(e))),
                Poll::Ready(None) => return this.finalize_stream(),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
