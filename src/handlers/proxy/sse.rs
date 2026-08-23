use core::time::Duration;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;

use futures_util::Stream;
use tokio_util::sync::WaitForCancellationFutureOwned;

use crate::db::{LogManager, ProxyEvent};
use crate::providers::UpstreamProvider;

use super::guard::{UsageTokens, parse_usage};

/// Cap for the event buffer. Standard SSE events are small; a buffer that
/// exceeds this without an event boundary is either an NDJSON-style stream
/// (split at the last newline) or a broken upstream (fail the stream).
const MAX_SSE_BUFFER_BYTES: usize = 1024 * 1024;

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
    pub(crate) idle_timeout: Duration,
    pub(crate) last_data_time: Instant,
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

    /// If the buffer exceeds the cap without an event boundary, split at the
    /// last newline (NDJSON-style streams) or fail if there is no newline at all.
    fn force_split_oversized(&mut self) -> Result<Option<usize>, String> {
        if self.buf.len() <= MAX_SSE_BUFFER_BYTES {
            return Ok(None);
        }
        match self.buf.iter().rposition(|&b| b == b'\n') {
            Some(i) => Ok(Some(i + 1)),
            None => Err("SSE buffer exceeded cap without a line boundary".to_string()),
        }
    }

    /// Fail the stream with a status and message, discarding the buffered bytes.
    fn fail_stream(
        &mut self,
        status: &str,
        msg: &str,
    ) -> Poll<Option<Result<bytes::Bytes, std::io::Error>>> {
        self.event.status = status.to_string();
        self.event.error_message = Some(msg.to_string());
        self.done = true;
        self.finalize_log();
        Poll::Ready(None)
    }

    fn split_event(&mut self, event_end: usize) -> bytes::Bytes {
        let event = self.buf.split_to(event_end);
        let transformed = self.transform_event(&event);
        self.event.response_body_size =
            Some(self.event.response_body_size.unwrap_or(0) + transformed.len() as i64);
        bytes::Bytes::from(transformed)
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
        self.event.latency_ms = self.start.elapsed().as_millis() as i64;
        self.log_manager.log_proxy(self.event.clone());
    }

    fn finalize_stream(&mut self) -> Poll<Option<Result<bytes::Bytes, std::io::Error>>> {
        self.done = true;
        self.record_ttfb();
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
            return Poll::Ready(Some(Ok(this.split_event(event_end))));
        }
        match this.force_split_oversized() {
            Ok(Some(event_end)) => return Poll::Ready(Some(Ok(this.split_event(event_end)))),
            Ok(None) => {}
            Err(msg) => {
                tracing::warn!(
                    model = this.event.model_name,
                    provider = this.event.provider_name,
                    "{}",
                    msg
                );
                return this.fail_stream("502", &msg);
            }
        }

        if Pin::new(&mut this.shutdown_fut).poll(cx).is_ready() {
            return this.finalize_stream();
        }

        loop {
            match Pin::new(&mut this.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    this.last_data_time = Instant::now();
                    this.buf.extend_from_slice(&bytes);
                    this.record_ttfb();
                    if let Some(event_end) = this.find_event_boundary() {
                        return Poll::Ready(Some(Ok(this.split_event(event_end))));
                    }
                    match this.force_split_oversized() {
                        Ok(Some(event_end)) => {
                            return Poll::Ready(Some(Ok(this.split_event(event_end))));
                        }
                        Ok(None) => {}
                        Err(msg) => {
                            tracing::warn!(
                                model = this.event.model_name,
                                provider = this.event.provider_name,
                                "{}",
                                msg
                            );
                            return this.fail_stream("502", &msg);
                        }
                    }
                }
                Poll::Ready(Some(Err(e))) => {
                    this.event.error_message = Some(e.to_string());
                    this.finalize_log();
                    this.done = true;
                    return Poll::Ready(Some(Err(e)));
                }
                Poll::Ready(None) => return this.finalize_stream(),
                Poll::Pending => {
                    if this.last_data_time.elapsed() >= this.idle_timeout {
                        this.event.status = "504".to_string();
                        this.event.error_message = Some("SSE stream idle timeout".to_string());
                        tracing::warn!(
                            model = this.event.model_name,
                            provider = this.event.provider_name,
                            idle_secs = this.last_data_time.elapsed().as_secs(),
                            "SSE stream idle timeout"
                        );
                        return this.finalize_stream();
                    }
                    return Poll::Pending;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::LogManager;
    use crate::providers::UpstreamProvider;
    use crate::test_utils::{create_test_state_fast_logs, make_proxy_event};
    use futures_util::stream;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio_util::sync::CancellationToken;

    struct MockUpstream;

    #[async_trait::async_trait]
    impl UpstreamProvider for MockUpstream {
        async fn build_request(
            &self,
            _client: &reqwest::Client,
            _body: serde_json::Value,
            _stream: bool,
            _upstream_model: &str,
            _upstream_path: &str,
        ) -> Result<reqwest::Request, String> {
            unreachable!()
        }

        fn transform_response(&self, body: &[u8], model_name: &str) -> Vec<u8> {
            let Ok(mut val) = serde_json::from_slice::<serde_json::Value>(body) else {
                return body.to_vec();
            };
            crate::providers::inject_default_shadow(&mut val, model_name);
            serde_json::to_vec(&val).unwrap_or_else(|_| body.to_vec())
        }
    }

    fn make_stream(
        inner: impl Stream<Item = Result<bytes::Bytes, std::io::Error>> + Unpin,
        log_manager: LogManager,
    ) -> SseTransformStream<impl Stream<Item = Result<bytes::Bytes, std::io::Error>>> {
        let token = CancellationToken::new();
        SseTransformStream {
            inner,
            buf: bytes::BytesMut::new(),
            upstream: Arc::new(MockUpstream),
            model_name: "test-model".to_string(),
            user_tokens: None,
            log_manager,
            event: make_proxy_event("test-model", "200", 0),
            start: Instant::now(),
            done: false,
            shutdown_fut: Box::pin(token.cancelled_owned()),
            idle_timeout: Duration::from_secs(60),
            last_data_time: Instant::now(),
        }
    }

    fn poll_stream<S: Stream<Item = Result<bytes::Bytes, std::io::Error>> + Unpin>(
        s: &mut S,
    ) -> Option<Result<bytes::Bytes, std::io::Error>> {
        let waker = futures_util::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        match Pin::new(s).poll_next(&mut cx) {
            Poll::Ready(item) => item,
            Poll::Pending => None,
        }
    }

    #[test]
    fn find_event_boundary_double_newline() {
        let (state, _dir) = create_test_state_fast_logs();
        let s = make_stream(
            stream::iter(Vec::<Result<bytes::Bytes, _>>::new()),
            state.log_manager,
        );
        let mut s = s;
        s.buf.extend_from_slice(b"data: hello\n\ndata: world\n\n");
        assert_eq!(s.find_event_boundary(), Some(13));
    }

    #[test]
    fn find_event_boundary_crlf() {
        let (state, _dir) = create_test_state_fast_logs();
        let s = make_stream(
            stream::iter(Vec::<Result<bytes::Bytes, _>>::new()),
            state.log_manager,
        );
        let mut s = s;
        s.buf.extend_from_slice(b"data: hello\r\n\r\ndata: world");
        assert_eq!(s.find_event_boundary(), Some(15));
    }

    #[test]
    fn find_event_boundary_no_boundary() {
        let (state, _dir) = create_test_state_fast_logs();
        let s = make_stream(
            stream::iter(Vec::<Result<bytes::Bytes, _>>::new()),
            state.log_manager,
        );
        let mut s = s;
        s.buf.extend_from_slice(b"data: hello without boundary");
        assert_eq!(s.find_event_boundary(), None);
    }

    #[test]
    fn transform_event_rewrites_data_lines() {
        let (state, _dir) = create_test_state_fast_logs();
        let s = make_stream(
            stream::iter(Vec::<Result<bytes::Bytes, _>>::new()),
            state.log_manager,
        );
        let mut s = s;
        let event = b"data: {\"choices\":[]}\n\n";
        let transformed = s.transform_event(event);
        let text = std::str::from_utf8(&transformed).unwrap();
        assert!(text.contains("data: "));
        assert!(text.contains("model"));
        assert!(text.contains("ait-proxy"));
    }

    #[test]
    fn transform_event_non_utf8_passthrough() {
        let (state, _dir) = create_test_state_fast_logs();
        let s = make_stream(
            stream::iter(Vec::<Result<bytes::Bytes, _>>::new()),
            state.log_manager,
        );
        let mut s = s;
        let event = &[0xFF, 0xFE, 0xFD];
        let transformed = s.transform_event(event);
        assert_eq!(transformed, event);
    }

    #[test]
    fn try_extract_usage_captures_tokens() {
        let (state, _dir) = create_test_state_fast_logs();
        let s = make_stream(
            stream::iter(Vec::<Result<bytes::Bytes, _>>::new()),
            state.log_manager,
        );
        let mut s = s;
        let payload = br#"{"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#;
        s.try_extract_usage_from(payload);
        assert_eq!(s.user_tokens.as_ref().unwrap().prompt_tokens, Some(10));
        assert_eq!(s.user_tokens.as_ref().unwrap().total_tokens, Some(15));
    }

    #[test]
    fn try_extract_usage_ignores_zero_tokens() {
        let (state, _dir) = create_test_state_fast_logs();
        let s = make_stream(
            stream::iter(Vec::<Result<bytes::Bytes, _>>::new()),
            state.log_manager,
        );
        let mut s = s;
        let payload = br#"{"usage":{"prompt_tokens":0,"completion_tokens":0,"total_tokens":0}}"#;
        s.try_extract_usage_from(payload);
        assert!(s.user_tokens.is_none());
    }

    #[test]
    fn force_split_oversized_under_cap_returns_none() {
        let (state, _dir) = create_test_state_fast_logs();
        let s = make_stream(
            stream::iter(Vec::<Result<bytes::Bytes, _>>::new()),
            state.log_manager,
        );
        let mut s = s;
        s.buf.extend_from_slice(b"small buffer");
        assert_eq!(s.force_split_oversized().unwrap(), None);
    }

    #[test]
    fn force_split_oversized_with_newline_splits() {
        let (state, _dir) = create_test_state_fast_logs();
        let s = make_stream(
            stream::iter(Vec::<Result<bytes::Bytes, _>>::new()),
            state.log_manager,
        );
        let mut s = s;
        s.buf
            .extend_from_slice(&vec![b'x'; MAX_SSE_BUFFER_BYTES + 100]);
        s.buf.extend_from_slice(b"\n");
        s.buf.extend_from_slice(b"remaining");
        let result = s.force_split_oversized().unwrap();
        assert!(result.is_some());
        assert!(result.unwrap() > MAX_SSE_BUFFER_BYTES);
    }

    #[test]
    fn force_split_oversized_no_newline_returns_error() {
        let (state, _dir) = create_test_state_fast_logs();
        let s = make_stream(
            stream::iter(Vec::<Result<bytes::Bytes, _>>::new()),
            state.log_manager,
        );
        let mut s = s;
        s.buf
            .extend_from_slice(&vec![b'x'; MAX_SSE_BUFFER_BYTES + 100]);
        assert!(s.force_split_oversized().is_err());
    }

    #[test]
    fn stream_processes_complete_event() {
        let (state, _dir) = create_test_state_fast_logs();
        let chunks: Vec<Result<bytes::Bytes, std::io::Error>> = vec![
            Ok(bytes::Bytes::from_static(b"data: {\"choices\":[]}\n\n")),
            Ok(bytes::Bytes::from_static(b"data: [DONE]\n\n")),
        ];
        let mut s = make_stream(stream::iter(chunks), state.log_manager);
        let item = poll_stream(&mut s);
        assert!(item.is_some());
        let chunk = item.unwrap().unwrap();
        let text = std::str::from_utf8(&chunk).unwrap();
        assert!(text.contains("data: "));
    }

    #[test]
    fn stream_finalizes_on_inner_end() {
        let (state, _dir) = create_test_state_fast_logs();
        let chunks: Vec<Result<bytes::Bytes, std::io::Error>> =
            vec![Ok(bytes::Bytes::from_static(b"data: {\"choices\":[]}\n\n"))];
        let mut s = make_stream(stream::iter(chunks), state.log_manager);
        let _first = poll_stream(&mut s);
        let second = poll_stream(&mut s);
        assert!(second.is_none());
        assert!(s.done);
    }

    #[test]
    fn stream_propagates_inner_error() {
        let (state, _dir) = create_test_state_fast_logs();
        let chunks: Vec<Result<bytes::Bytes, std::io::Error>> = vec![Err(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "upstream gone",
        ))];
        let mut s = make_stream(stream::iter(chunks), state.log_manager);
        let item = poll_stream(&mut s);
        assert!(item.is_some());
        assert!(item.unwrap().is_err());
        assert!(s.done);
    }

    #[test]
    fn drop_without_done_writes_499_log() {
        let (state, _dir) = create_test_state_fast_logs();
        let chunks: Vec<Result<bytes::Bytes, std::io::Error>> = vec![];
        let s = make_stream(stream::iter(chunks), state.log_manager);
        assert!(!s.done);
        drop(s);
    }

    #[test]
    fn stream_fail_on_oversized_buffer_without_newline() {
        let (state, _dir) = create_test_state_fast_logs();
        // Feed a single chunk larger than MAX_SSE_BUFFER_BYTES with no newline,
        // so force_split_oversized returns Err and poll_next calls fail_stream.
        let big = bytes::Bytes::from(vec![b'x'; MAX_SSE_BUFFER_BYTES + 100]);
        let chunks: Vec<Result<bytes::Bytes, std::io::Error>> = vec![Ok(big)];
        let mut s = make_stream(stream::iter(chunks), state.log_manager);
        let item = poll_stream(&mut s);
        assert!(item.is_none());
        assert!(s.done);
        assert_eq!(s.event.status, "502");
        assert!(s.event.error_message.is_some());
    }

    #[test]
    fn finalize_stream_flushes_remaining_buffer() {
        let (state, _dir) = create_test_state_fast_logs();
        // Feed an event followed by an incomplete chunk (no trailing newline).
        // The first poll yields the complete event; the second poll hits
        // Poll::Ready(None) on the inner stream and finalize_stream should
        // flush the remaining buffer as a final chunk.
        let chunks: Vec<Result<bytes::Bytes, std::io::Error>> = vec![
            Ok(bytes::Bytes::from_static(b"data: {\"choices\":[]}\n\n")),
            Ok(bytes::Bytes::from_static(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}",
            )),
        ];
        let mut s = make_stream(stream::iter(chunks), state.log_manager);
        let first = poll_stream(&mut s);
        assert!(first.is_some());
        let second = poll_stream(&mut s);
        assert!(second.is_some());
        let chunk = second.unwrap().unwrap();
        let text = std::str::from_utf8(&chunk).unwrap();
        assert!(text.contains("data: "));
        assert!(s.done);
    }
}
