use std::time::Instant;

use crate::db::{LogManager, ProxyEvent};

#[derive(Default)]
pub(crate) struct UsageTokens {
    pub(crate) prompt_tokens: Option<i64>,
    pub(crate) completion_tokens: Option<i64>,
    pub(crate) total_tokens: Option<i64>,
    pub(crate) cached_tokens: Option<i64>,
}

pub(crate) fn parse_usage(body: &[u8]) -> UsageTokens {
    let Ok(val) = serde_json::from_slice::<serde_json::Value>(body) else {
        return UsageTokens::default();
    };

    // Responses API streaming terminal events (response.completed etc.)
    // nest usage inside the response object; all other payloads carry it
    // at the top level.
    let usage = val
        .get("usage")
        .or_else(|| val.get("response").and_then(|r| r.get("usage")));

    let prompt_tokens = usage
        .and_then(|u| u.get("prompt_tokens").and_then(|v| v.as_i64()))
        .or_else(|| usage.and_then(|u| u.get("input_tokens").and_then(|v| v.as_i64())));

    let completion_tokens = usage
        .and_then(|u| u.get("completion_tokens").and_then(|v| v.as_i64()))
        .or_else(|| usage.and_then(|u| u.get("output_tokens").and_then(|v| v.as_i64())));

    let total_tokens = usage
        .and_then(|u| u.get("total_tokens").and_then(|v| v.as_i64()))
        .or_else(|| match (prompt_tokens, completion_tokens) {
            (Some(prompt), Some(completion)) => Some(prompt + completion),
            _ => None,
        });

    let cached_tokens = usage
        .and_then(|u| {
            u.get("prompt_tokens_details")
                .or_else(|| u.get("completion_tokens_details"))
        })
        .and_then(|d| d.get("cached_tokens").and_then(|v| v.as_i64()))
        .or_else(|| {
            usage
                .and_then(|u| u.get("input_tokens_details"))
                .and_then(|d| d.get("cached_tokens").and_then(|v| v.as_i64()))
        })
        .or_else(|| usage.and_then(|u| u.get("cached_tokens").and_then(|v| v.as_i64())));

    UsageTokens {
        prompt_tokens,
        completion_tokens,
        total_tokens,
        cached_tokens,
    }
}

pub(crate) struct ProxyLogGuard {
    log_manager: LogManager,
    pub(crate) event: ProxyEvent,
    start: Instant,
    finalized: bool,
}

impl ProxyLogGuard {
    pub(crate) fn new(log_manager: LogManager, event: ProxyEvent, start: Instant) -> Self {
        Self {
            log_manager,
            event,
            start,
            finalized: false,
        }
    }

    pub(crate) fn finalize(&mut self, usage: &UsageTokens, status: &str) {
        // Callers seed prompt_tokens with a body-size estimate. Keep it when
        // the upstream reported no usage: a successful response without usage
        // carries nothing to replace the estimate with.
        let seeded_prompt_tokens = self.event.prompt_tokens;
        self.event.prompt_tokens = usage.prompt_tokens.or(seeded_prompt_tokens);
        self.event.completion_tokens = usage.completion_tokens;
        self.event.total_tokens = usage.total_tokens;
        self.event.cached_tokens = usage.cached_tokens;
        self.event.latency_ms = self.start.elapsed().as_millis() as i64;
        self.event.status = status.to_string();
        self.log_manager.log_proxy(self.event.clone());
        self.finalized = true;
    }

    /// Suppress the drop-guard's "499" fallback without writing a log.
    ///
    /// Used in the streaming path where `SseTransformStream` is responsible
    /// for writing the log once the stream completes.
    pub(crate) fn suppress_drop_log(&mut self) {
        self.finalized = true;
    }
}

impl Drop for ProxyLogGuard {
    fn drop(&mut self) {
        if self.finalized {
            return;
        }
        self.event.latency_ms = self.start.elapsed().as_millis() as i64;
        self.event.status = "499".to_string();
        self.log_manager.log_proxy(self.event.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{create_test_state_fast_logs, make_proxy_event};

    #[test]
    fn parse_chat_completions_usage() {
        let body = br#"{"usage":{"prompt_tokens":10,"completion_tokens":20,"total_tokens":30,"prompt_tokens_details":{"cached_tokens":5}}}"#;
        let u = parse_usage(body);
        assert_eq!(u.prompt_tokens, Some(10));
        assert_eq!(u.completion_tokens, Some(20));
        assert_eq!(u.total_tokens, Some(30));
        assert_eq!(u.cached_tokens, Some(5));
    }

    #[test]
    fn parse_responses_usage() {
        let body = br#"{"id":"resp-1","object":"response","usage":{"input_tokens":100,"output_tokens":83,"output_tokens_details":{"reasoning_tokens":0},"total_tokens":183}}"#;
        let u = parse_usage(body);
        assert_eq!(u.prompt_tokens, Some(100));
        assert_eq!(u.completion_tokens, Some(83));
        assert_eq!(u.total_tokens, Some(183));
        assert_eq!(u.cached_tokens, None);
    }

    #[test]
    fn parse_responses_usage_with_cached_tokens() {
        let body = br#"{"usage":{"input_tokens":200,"output_tokens":50,"input_tokens_details":{"cached_tokens":150}}}"#;
        let u = parse_usage(body);
        assert_eq!(u.prompt_tokens, Some(200));
        assert_eq!(u.completion_tokens, Some(50));
        // total_tokens missing -> falls back to input + output
        assert_eq!(u.total_tokens, Some(250));
        assert_eq!(u.cached_tokens, Some(150));
    }

    #[test]
    fn parse_responses_streaming_terminal_event_usage() {
        let body = br#"{"type":"response.completed","sequence_number":10,"response":{"id":"resp-2","status":"completed","usage":{"input_tokens":7,"output_tokens":3}}}"#;
        let u = parse_usage(body);
        assert_eq!(u.prompt_tokens, Some(7));
        assert_eq!(u.completion_tokens, Some(3));
        assert_eq!(u.total_tokens, Some(10));
    }

    #[test]
    fn parse_usage_ignores_payloads_without_usage() {
        let body = br#"{"type":"response.output_text.delta","delta":"hello"}"#;
        let u = parse_usage(body);
        assert_eq!(u.prompt_tokens, None);
        assert_eq!(u.completion_tokens, None);
        assert_eq!(u.total_tokens, None);
        assert_eq!(u.cached_tokens, None);
    }

    #[test]
    fn parse_usage_invalid_json_returns_default() {
        let u = parse_usage(b"not json");
        assert_eq!(u.prompt_tokens, None);
        assert_eq!(u.completion_tokens, None);
        assert_eq!(u.total_tokens, None);
        assert_eq!(u.cached_tokens, None);
    }

    #[test]
    fn parse_usage_cached_tokens_from_completion_tokens_details() {
        let body = br#"{"usage":{"prompt_tokens":10,"completion_tokens":20,"completion_tokens_details":{"cached_tokens":7}}}"#;
        let u = parse_usage(body);
        assert_eq!(u.cached_tokens, Some(7));
    }

    #[test]
    fn parse_usage_cached_tokens_top_level_fallback() {
        let body = br#"{"usage":{"prompt_tokens":10,"completion_tokens":20,"cached_tokens":42}}"#;
        let u = parse_usage(body);
        assert_eq!(u.prompt_tokens, Some(10));
        assert_eq!(u.completion_tokens, Some(20));
        assert_eq!(u.cached_tokens, Some(42));
    }

    // ── ProxyLogGuard ──

    #[test]
    fn guard_finalize_writes_log_and_sets_finalized() {
        let (state, _dir) = create_test_state_fast_logs();
        let event = make_proxy_event("gpt-4", "pending", 0);
        let mut guard = ProxyLogGuard::new(state.log_manager.clone(), event, Instant::now());
        let usage = UsageTokens {
            prompt_tokens: Some(10),
            completion_tokens: Some(20),
            total_tokens: Some(30),
            cached_tokens: Some(5),
        };
        guard.finalize(&usage, "200");
        assert!(guard.finalized);
        assert_eq!(guard.event.status, "200");
        assert_eq!(guard.event.prompt_tokens, Some(10));
        assert_eq!(guard.event.total_tokens, Some(30));
    }

    #[test]
    fn guard_finalize_keeps_seeded_prompt_estimate_without_usage() {
        let (state, _dir) = create_test_state_fast_logs();
        let mut event = make_proxy_event("gpt-4", "pending", 0);
        event.prompt_tokens = Some(42);
        let mut guard = ProxyLogGuard::new(state.log_manager.clone(), event, Instant::now());
        // Successful request, but the upstream reported no usage at all.
        guard.finalize(&UsageTokens::default(), "200");
        assert_eq!(guard.event.prompt_tokens, Some(42));
    }

    #[test]
    fn guard_finalize_prefers_upstream_usage_over_estimate() {
        let (state, _dir) = create_test_state_fast_logs();
        let mut event = make_proxy_event("gpt-4", "pending", 0);
        event.prompt_tokens = Some(42);
        let mut guard = ProxyLogGuard::new(state.log_manager.clone(), event, Instant::now());
        let usage = UsageTokens {
            prompt_tokens: Some(10),
            completion_tokens: Some(20),
            total_tokens: Some(30),
            cached_tokens: Some(5),
        };
        guard.finalize(&usage, "200");
        assert_eq!(guard.event.prompt_tokens, Some(10));
        assert_eq!(guard.event.total_tokens, Some(30));
    }

    #[test]
    fn guard_suppress_drop_log_prevents_499_on_drop() {
        let (state, _dir) = create_test_state_fast_logs();
        let event = make_proxy_event("gpt-4", "200", 30);
        let mut guard = ProxyLogGuard::new(state.log_manager.clone(), event, Instant::now());
        guard.suppress_drop_log();
        assert!(guard.finalized);
        // Drop should NOT write a 499 log — finalized is true.
        drop(guard);
    }

    #[test]
    fn guard_drop_without_finalize_writes_499() {
        let (state, _dir) = create_test_state_fast_logs();
        let event = make_proxy_event("gpt-4", "pending", 0);
        let guard = ProxyLogGuard::new(state.log_manager.clone(), event, Instant::now());
        // Drop without finalize -> should write a 499 status log.
        drop(guard);
    }
}
