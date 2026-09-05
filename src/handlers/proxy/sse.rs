use core::time::Duration;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;

use futures_util::Stream;
use tokio::time::Sleep;
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
    /// Bytes of `buf` already scanned for an event boundary. Each scan only
    /// inspects the tail beyond this offset (keeping the last 3 bytes
    /// unscanned so a `\r\n\r\n` boundary straddling two chunks is not
    /// missed), which keeps the search linear in new bytes instead of
    /// rescanning the whole buffer on every poll.
    pub(crate) scanned: usize,
    pub(crate) upstream: Arc<dyn UpstreamProvider>,
    pub(crate) model_name: String,
    pub(crate) user_tokens: Option<UsageTokens>,
    pub(crate) log_manager: LogManager,
    pub(crate) event: ProxyEvent,
    pub(crate) start: Instant,
    pub(crate) shutdown_fut: Pin<Box<WaitForCancellationFutureOwned>>,
    pub(crate) done: bool,
    pub(crate) idle_timeout: Duration,
    /// Wakes the task when the idle deadline expires. Without it a silent
    /// upstream leaves the task parked forever, because returning
    /// `Poll::Pending` from the inner stream does not re-poll this one.
    pub(crate) idle_timer: Pin<Box<Sleep>>,
    /// Hard cap on total stream lifetime; an upstream that trickles a byte
    /// before each idle deadline would otherwise hold the connection open
    /// indefinitely.
    pub(crate) max_duration: Duration,
    /// Cumulative bytes forwarded, capped so a long-lived stream cannot exceed
    /// the configured response size limit across many events.
    pub(crate) total_bytes: usize,
    pub(crate) max_bytes: usize,
}

impl<S> SseTransformStream<S> {
    fn find_event_boundary(&mut self) -> Option<usize> {
        let start = self.scanned.min(self.buf.len());
        let hay = &self.buf[start..];
        if let Some(i) = hay.windows(4).position(|w| w == b"\r\n\r\n") {
            return Some(start + i + 4);
        }
        if let Some(i) = hay.windows(2).position(|w| w == b"\n\n") {
            return Some(start + i + 2);
        }
        // Nothing found: mark the scan position, keeping the last 3 bytes
        // unmarked so a boundary completed by the next chunk is still seen.
        self.scanned = self.buf.len().saturating_sub(3);
        None
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
        self.scanned = 0;
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

    /// Re-arm the idle deadline. Called whenever upstream data arrives; the
    /// timer is what actually wakes this task when upstream goes silent.
    fn reset_idle_timer(&mut self) {
        let deadline = tokio::time::Instant::from_std(Instant::now() + self.idle_timeout);
        self.idle_timer.as_mut().reset(deadline);
    }

    /// End the stream with a 504, recording `msg` as the failure reason.
    fn timeout_stream(&mut self, msg: &str) -> Poll<Option<Result<bytes::Bytes, std::io::Error>>> {
        self.event.status = "504".to_string();
        self.event.error_message = Some(msg.to_string());
        tracing::warn!(
            model = self.event.model_name,
            provider = self.event.provider_name,
            "{}",
            msg
        );
        self.finalize_stream()
    }

    fn finalize_log(&mut self) {
        if let Some(usage) = self.user_tokens.take() {
            // Same seeding rule as ProxyLogGuard::finalize: the request path
            // seeds prompt_tokens with a body-size estimate, so an upstream
            // that reports usage without a prompt count must not erase it.
            let seeded = self.event.prompt_tokens;
            self.event.prompt_tokens = usage.prompt_tokens.or(seeded);
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

        // Byte buffer, not String: `transform_response` already yields bytes,
        // so this avoids a utf8 round trip and a final whole-buffer copy.
        let mut out = Vec::with_capacity(event.len() + 64);
        for line in text.lines() {
            // SSE allows `data:` with or without one separating space; the
            // space is not part of the payload, and upstreams emit both forms.
            let payload = line
                .strip_prefix("data:")
                .map(|rest| rest.strip_prefix(' ').unwrap_or(rest));
            if let Some(payload) = payload {
                self.try_extract_usage_from(payload.as_bytes());
                let transformed = self
                    .upstream
                    .transform_response(payload.as_bytes(), &self.model_name);
                out.extend_from_slice(b"data: ");
                out.extend_from_slice(&transformed);
                out.push(b'\n');
            } else if !line.is_empty() || !out.ends_with(b"\n\n") {
                out.extend_from_slice(line.as_bytes());
                out.push(b'\n');
            }
        }
        out
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

        if this.start.elapsed() >= this.max_duration {
            return this.timeout_stream("SSE stream exceeded max duration");
        }

        // The idle deadline can only fire if something wakes this task, so the
        // timer is polled on every turn. Returning Pending below leaves the
        // timer as the sole wake-up source for a silent upstream.
        if Pin::new(&mut this.idle_timer).poll(cx).is_ready() {
            return this.timeout_stream("SSE stream idle timeout");
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
                    this.reset_idle_timer();
                    this.total_bytes += bytes.len();
                    if this.total_bytes > this.max_bytes {
                        return this.fail_stream("502", "SSE stream exceeded max response size");
                    }
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
                // The idle timer polled above is the wake-up source here;
                // re-checking elapsed time would never run without it.
                Poll::Pending => return Poll::Pending,
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
    use tokio::time::sleep;
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
        make_stream_with(
            inner,
            log_manager,
            Duration::from_secs(60),
            Duration::MAX,
            usize::MAX,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn make_stream_with(
        inner: impl Stream<Item = Result<bytes::Bytes, std::io::Error>> + Unpin,
        log_manager: LogManager,
        idle_timeout: Duration,
        max_duration: Duration,
        max_bytes: usize,
    ) -> SseTransformStream<impl Stream<Item = Result<bytes::Bytes, std::io::Error>>> {
        let token = CancellationToken::new();
        SseTransformStream {
            inner,
            buf: bytes::BytesMut::new(),
            scanned: 0,
            upstream: Arc::new(MockUpstream),
            model_name: "test-model".to_string(),
            user_tokens: None,
            log_manager,
            event: make_proxy_event("test-model", "200", 0),
            start: Instant::now(),
            done: false,
            shutdown_fut: Box::pin(token.cancelled_owned()),
            idle_timeout,
            idle_timer: Box::pin(sleep(idle_timeout)),
            max_duration,
            total_bytes: 0,
            max_bytes,
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

    #[tokio::test]
    async fn find_event_boundary_double_newline() {
        let (state, _dir) = create_test_state_fast_logs();
        let s = make_stream(
            stream::iter(Vec::<Result<bytes::Bytes, _>>::new()),
            state.log_manager,
        );
        let mut s = s;
        s.buf.extend_from_slice(b"data: hello\n\ndata: world\n\n");
        assert_eq!(s.find_event_boundary(), Some(13));
    }

    #[tokio::test]
    async fn find_event_boundary_crlf() {
        let (state, _dir) = create_test_state_fast_logs();
        let s = make_stream(
            stream::iter(Vec::<Result<bytes::Bytes, _>>::new()),
            state.log_manager,
        );
        let mut s = s;
        s.buf.extend_from_slice(b"data: hello\r\n\r\ndata: world");
        assert_eq!(s.find_event_boundary(), Some(15));
    }

    #[tokio::test]
    async fn find_event_boundary_no_boundary() {
        let (state, _dir) = create_test_state_fast_logs();
        let s = make_stream(
            stream::iter(Vec::<Result<bytes::Bytes, _>>::new()),
            state.log_manager,
        );
        let mut s = s;
        s.buf.extend_from_slice(b"data: hello without boundary");
        assert_eq!(s.find_event_boundary(), None);
    }

    #[tokio::test]
    async fn find_event_boundary_crlf_straddling_chunks() {
        let (state, _dir) = create_test_state_fast_logs();
        let s = make_stream(
            stream::iter(Vec::<Result<bytes::Bytes, _>>::new()),
            state.log_manager,
        );
        let mut s = s;
        // A \r\n\r\n completed by the second chunk must still be found even
        // though the first scan already advanced the cursor past its start.
        s.buf.extend_from_slice(b"ab\r\n");
        assert_eq!(s.find_event_boundary(), None);
        s.buf.extend_from_slice(b"\r\nx");
        assert_eq!(s.find_event_boundary(), Some(6));
    }

    #[tokio::test]
    async fn find_event_boundary_lf_straddling_chunks() {
        let (state, _dir) = create_test_state_fast_logs();
        let s = make_stream(
            stream::iter(Vec::<Result<bytes::Bytes, _>>::new()),
            state.log_manager,
        );
        let mut s = s;
        s.buf.extend_from_slice(b"a\n");
        assert_eq!(s.find_event_boundary(), None);
        s.buf.extend_from_slice(b"\nb");
        assert_eq!(s.find_event_boundary(), Some(3));
    }

    #[tokio::test]
    async fn transform_event_rewrites_data_lines() {
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

    #[tokio::test]
    async fn transform_event_rewrites_data_lines_without_space() {
        let (state, _dir) = create_test_state_fast_logs();
        let s = make_stream(
            stream::iter(Vec::<Result<bytes::Bytes, _>>::new()),
            state.log_manager,
        );
        let mut s = s;
        // SSE permits `data:` with no separating space; upstreams emit both
        // forms and both carry a payload that must be transformed.
        let event = b"data:{\"choices\":[]}\n\n";
        let transformed = s.transform_event(event);
        let text = std::str::from_utf8(&transformed).unwrap();
        assert!(
            text.contains("ait-proxy"),
            "payload must be transformed: {text}"
        );
    }

    #[tokio::test]
    async fn transform_event_non_utf8_passthrough() {
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

    #[tokio::test]
    async fn try_extract_usage_captures_tokens() {
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

    #[tokio::test]
    async fn try_extract_usage_ignores_zero_tokens() {
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

    #[tokio::test]
    async fn finalize_log_keeps_seeded_prompt_estimate_without_upstream_prompt() {
        let (state, _dir) = create_test_state_fast_logs();
        let mut s = make_stream(
            stream::iter(Vec::<Result<bytes::Bytes, std::io::Error>>::new()),
            state.log_manager,
        );
        // The request path seeds an estimate; the upstream reports usage with
        // only a completion count.
        s.event.prompt_tokens = Some(42);
        s.try_extract_usage_from(br#"{"usage":{"completion_tokens":5,"total_tokens":5}}"#);
        s.finalize_log();
        s.done = true;
        assert_eq!(s.event.prompt_tokens, Some(42));
        assert_eq!(s.event.completion_tokens, Some(5));
    }

    #[tokio::test]
    async fn force_split_oversized_under_cap_returns_none() {
        let (state, _dir) = create_test_state_fast_logs();
        let s = make_stream(
            stream::iter(Vec::<Result<bytes::Bytes, _>>::new()),
            state.log_manager,
        );
        let mut s = s;
        s.buf.extend_from_slice(b"small buffer");
        assert_eq!(s.force_split_oversized().unwrap(), None);
    }

    #[tokio::test]
    async fn force_split_oversized_with_newline_splits() {
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

    #[tokio::test]
    async fn force_split_oversized_no_newline_returns_error() {
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

    #[tokio::test]
    async fn stream_processes_complete_event() {
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

    #[tokio::test]
    async fn stream_finalizes_on_inner_end() {
        let (state, _dir) = create_test_state_fast_logs();
        let chunks: Vec<Result<bytes::Bytes, std::io::Error>> =
            vec![Ok(bytes::Bytes::from_static(b"data: {\"choices\":[]}\n\n"))];
        let mut s = make_stream(stream::iter(chunks), state.log_manager);
        let _first = poll_stream(&mut s);
        let second = poll_stream(&mut s);
        assert!(second.is_none());
        assert!(s.done);
    }

    #[tokio::test]
    async fn stream_propagates_inner_error() {
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

    #[tokio::test]
    async fn drop_without_done_writes_499_log() {
        let (state, _dir) = create_test_state_fast_logs();
        let chunks: Vec<Result<bytes::Bytes, std::io::Error>> = vec![];
        let s = make_stream(stream::iter(chunks), state.log_manager);
        assert!(!s.done);
        drop(s);
    }

    #[tokio::test]
    async fn stream_fail_on_oversized_buffer_without_newline() {
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

    #[tokio::test]
    async fn finalize_stream_flushes_remaining_buffer() {
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

    #[tokio::test]
    async fn stream_idle_timer_ends_silent_upstream() {
        let (state, _dir) = create_test_state_fast_logs();
        // A silent upstream leaves the task parked after Pending; only the idle
        // timer can wake it again.
        let mut s = make_stream_with(
            stream::pending::<Result<bytes::Bytes, std::io::Error>>(),
            state.log_manager,
            Duration::from_millis(50),
            Duration::MAX,
            usize::MAX,
        );
        assert!(poll_stream(&mut s).is_none());
        assert!(!s.done, "stream must still be open before the deadline");

        tokio::time::sleep(Duration::from_millis(120)).await;

        assert!(poll_stream(&mut s).is_none());
        assert!(s.done);
        assert_eq!(s.event.status, "504");
    }

    #[tokio::test]
    async fn stream_exceeding_max_duration_is_cut_off() {
        let (state, _dir) = create_test_state_fast_logs();
        let chunks: Vec<Result<bytes::Bytes, std::io::Error>> =
            vec![Ok(bytes::Bytes::from_static(b"data: {\"choices\":[]}\n\n"))];
        let mut s = make_stream_with(
            stream::iter(chunks),
            state.log_manager,
            Duration::from_secs(60),
            Duration::ZERO,
            usize::MAX,
        );
        assert!(poll_stream(&mut s).is_none());
        assert!(s.done);
        assert_eq!(s.event.status, "504");
        assert!(
            s.event
                .error_message
                .as_deref()
                .is_some_and(|m| m.contains("max duration"))
        );
    }

    #[tokio::test]
    async fn stream_exceeding_max_bytes_is_cut_off() {
        let (state, _dir) = create_test_state_fast_logs();
        let chunks: Vec<Result<bytes::Bytes, std::io::Error>> =
            vec![Ok(bytes::Bytes::from_static(b"data: {\"choices\":[]}\n\n"))];
        let mut s = make_stream_with(
            stream::iter(chunks),
            state.log_manager,
            Duration::from_secs(60),
            Duration::MAX,
            1,
        );
        assert!(poll_stream(&mut s).is_none());
        assert!(s.done);
        assert_eq!(s.event.status, "502");
    }

    #[tokio::test]
    async fn stream_data_resets_idle_timer() {
        let (state, _dir) = create_test_state_fast_logs();
        // Two chunks separated by a wait shorter than the idle timeout: the
        // stream must survive because each chunk re-arms the deadline.
        let mut s = make_stream_with(
            stream::iter(vec![
                Ok(bytes::Bytes::from_static(b"data: {\"choices\":[]}\n\n")),
                Ok(bytes::Bytes::from_static(b"data: [DONE]\n\n")),
            ]),
            state.log_manager,
            Duration::from_millis(200),
            Duration::MAX,
            usize::MAX,
        );
        assert!(poll_stream(&mut s).is_some());
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert!(poll_stream(&mut s).is_some());
        assert!(!s.done);
    }
}
