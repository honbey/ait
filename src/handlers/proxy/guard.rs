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

    let usage = val.get("usage");

    let prompt_tokens = usage.and_then(|u| u.get("prompt_tokens").and_then(|v| v.as_i64()));

    let completion_tokens = usage.and_then(|u| u.get("completion_tokens").and_then(|v| v.as_i64()));

    let total_tokens = usage.and_then(|u| u.get("total_tokens").and_then(|v| v.as_i64()));

    let cached_tokens = usage
        .and_then(|u| {
            u.get("prompt_tokens_details")
                .or_else(|| u.get("completion_tokens_details"))
        })
        .and_then(|d| d.get("cached_tokens").and_then(|v| v.as_i64()))
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
    event: ProxyEvent,
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
        self.event.prompt_tokens = usage.prompt_tokens;
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
