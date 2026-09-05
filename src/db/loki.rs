use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use tracing::warn;

use super::models::LogEvent;
use crate::config::LokiConfig;

const LOKI_CHANNEL_CAP: usize = 4096;

#[derive(Clone)]
pub struct LokiSink {
    sender: mpsc::SyncSender<LogEvent>,
    worker_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
}

struct WorkerState {
    client: reqwest::blocking::Client,
    push_url: String,
    static_labels: HashMap<String, String>,
    basic_auth_user: Option<String>,
    basic_auth_password: Option<String>,
    bearer_token: Option<String>,
}

#[derive(Debug)]
pub enum LokiInitError {
    /// Push URL is not a valid absolute http(s) URL.
    InvalidUrl(String),
    /// reqwest blocking client failed to build.
    Client(reqwest::Error),
    /// Only one half of the basic-auth pair was configured.
    IncompleteBasicAuth,
}

impl std::fmt::Display for LokiInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidUrl(url) => write!(f, "invalid Loki url: {url}"),
            Self::Client(e) => write!(f, "HTTP client init failed: {e}"),
            Self::IncompleteBasicAuth => write!(
                f,
                "basic_auth_user and basic_auth_password must be set together (or neither)"
            ),
        }
    }
}

impl std::error::Error for LokiInitError {}

/// Check a label name against the Loki push API naming rule
/// `[a-zA-Z_][a-zA-Z0-9_]*`.
fn is_valid_label_name(name: &str) -> bool {
    !name.is_empty()
        && name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Drop static labels whose names would make every push fail with HTTP 400.
fn sanitize_static_labels(raw: &HashMap<String, String>) -> HashMap<String, String> {
    let mut valid = HashMap::new();
    for (k, v) in raw {
        if is_valid_label_name(k) {
            valid.insert(k.clone(), v.clone());
        } else {
            warn!("[loki] dropping invalid label name {k:?} (must match [a-zA-Z_][a-zA-Z0-9_]*)");
        }
    }
    valid
}

impl LokiSink {
    pub fn new(config: &LokiConfig) -> Result<Self, LokiInitError> {
        let push_url = format!("{}/loki/api/v1/push", config.url.trim_end_matches('/'));
        match reqwest::Url::parse(&push_url) {
            Ok(u) if matches!(u.scheme(), "http" | "https") => {}
            _ => return Err(LokiInitError::InvalidUrl(push_url)),
        }

        // Refuse a half-configured pair: sending events with no auth header on
        // a typo is worse than not sending them. Config load rejects this too;
        // this keeps the invariant for any other caller.
        match (&config.basic_auth_user, &config.basic_auth_password) {
            (Some(_), Some(_)) | (None, None) => {}
            _ => return Err(LokiInitError::IncompleteBasicAuth),
        }

        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(LokiInitError::Client)?;

        let state = WorkerState {
            client,
            push_url,
            static_labels: sanitize_static_labels(&config.labels),
            basic_auth_user: config.basic_auth_user.clone(),
            basic_auth_password: config.basic_auth_password.clone(),
            bearer_token: config.bearer_token.clone(),
        };
        let batch_size = config.batch_size.max(1);
        let interval = Duration::from_secs(config.interval_secs.max(1));

        let (sender, receiver) = mpsc::sync_channel(LOKI_CHANNEL_CAP);
        let handle = thread::spawn(move || {
            loki_worker(receiver, state, batch_size, interval);
        });

        Ok(Self {
            sender,
            worker_handle: Arc::new(Mutex::new(Some(handle))),
        })
    }

    pub fn send(&self, event: LogEvent) {
        if let Err(e) = self.sender.try_send(event) {
            warn!("[loki] buffer full, dropping event: {e}");
        }
    }

    pub fn shutdown(&self) {
        let signaled = self.signal_shutdown();
        // Only join when the worker actually received the signal; joining one
        // that never did would hang.
        if signaled
            && let Ok(mut guard) = self.worker_handle.lock()
            && let Some(handle) = guard.take()
        {
            let _ = handle.join();
        }
    }

    /// Hand the worker a shutdown event, retrying while the channel is full.
    /// `send` would block indefinitely on a full channel, so the wait is
    /// bounded; returns false when the signal could not be delivered.
    fn signal_shutdown(&self) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match self.sender.try_send(LogEvent::Shutdown) {
                Ok(()) => return true,
                Err(mpsc::TrySendError::Disconnected(_)) => return false,
                Err(mpsc::TrySendError::Full(_)) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(mpsc::TrySendError::Full(_)) => {
                    warn!("[loki] shutdown signal not delivered; worker left running");
                    return false;
                }
            }
        }
    }
}

fn loki_worker(
    receiver: mpsc::Receiver<LogEvent>,
    state: WorkerState,
    batch_size: u64,
    interval: Duration,
) {
    let mut buffer: Vec<LogEvent> = Vec::with_capacity(batch_size as usize);
    let mut consecutive_failures: u64 = 0;

    loop {
        let mut shutdown = false;

        match receiver.recv_timeout(interval) {
            Ok(LogEvent::Shutdown) => shutdown = true,
            Ok(event) => buffer.push(event),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !buffer.is_empty() {
                    flush_loki(&state, &mut buffer, &mut consecutive_failures);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if !buffer.is_empty() {
                    flush_loki(&state, &mut buffer, &mut consecutive_failures);
                }
                return;
            }
        }

        while let Ok(event) = receiver.try_recv() {
            match event {
                LogEvent::Shutdown => shutdown = true,
                other => buffer.push(other),
            }
        }

        if (buffer.len() as u64) >= batch_size {
            flush_loki(&state, &mut buffer, &mut consecutive_failures);
        }

        if shutdown {
            if !buffer.is_empty() {
                flush_loki(&state, &mut buffer, &mut consecutive_failures);
            }
            return;
        }
    }
}

fn flush_loki(state: &WorkerState, buffer: &mut Vec<LogEvent>, consecutive_failures: &mut u64) {
    let payload = build_push_payload(buffer, &state.static_labels);
    buffer.clear();

    let mut req = state
        .client
        .post(&state.push_url)
        .header("Content-Type", "application/json")
        .body(payload.to_string());

    if let (Some(user), Some(pass)) = (&state.basic_auth_user, &state.basic_auth_password) {
        req = req.basic_auth(user, Some(pass));
    }
    if let Some(token) = &state.bearer_token {
        req = req.bearer_auth(token);
    }

    match req.send() {
        Ok(resp) if resp.status().is_success() => {
            *consecutive_failures = 0;
        }
        Ok(resp) => {
            *consecutive_failures += 1;
            if *consecutive_failures == 1 || consecutive_failures.is_multiple_of(10) {
                warn!(
                    "[loki] push failed: HTTP {} (consecutive failures: {})",
                    resp.status(),
                    consecutive_failures
                );
            }
        }
        Err(e) => {
            *consecutive_failures += 1;
            if *consecutive_failures == 1 || consecutive_failures.is_multiple_of(10) {
                warn!(
                    "[loki] push error: {e} (consecutive failures: {})",
                    consecutive_failures
                );
            }
        }
    }
}

// ── Pure functions (testable without HTTP) ──

/// Extract the timestamp from a LogEvent.
fn event_timestamp(event: &LogEvent) -> DateTime<Utc> {
    match event {
        LogEvent::Access(e) => e.timestamp,
        LogEvent::Proxy(e) => e.timestamp,
        LogEvent::Audit(e) => e.timestamp,
        LogEvent::Shutdown => Utc::now(),
    }
}

/// Compute Loki labels for an event: static labels from config plus
/// bounded-cardinality fields only; unbounded fields stay in the JSON line.
fn event_to_labels(
    event: &LogEvent,
    static_labels: &HashMap<String, String>,
) -> BTreeMap<String, String> {
    let mut labels: BTreeMap<String, String> = static_labels
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    match event {
        LogEvent::Access(e) => {
            labels.insert("event_type".into(), "access".into());
            labels.insert("method".into(), e.method.clone());
        }
        LogEvent::Proxy(e) => {
            labels.insert("event_type".into(), "proxy".into());
            labels.insert("model_name".into(), e.model_name.clone());
            labels.insert("status".into(), e.status.clone());
            labels.insert("is_streaming".into(), e.is_streaming.to_string());
        }
        LogEvent::Audit(e) => {
            labels.insert("event_type".into(), "audit".into());
            labels.insert("action".into(), e.action.clone());
            labels.insert("resource".into(), e.resource.clone());
        }
        LogEvent::Shutdown => {}
    }
    labels
}

/// Serialize an event as a JSON log line.
fn event_to_line(event: &LogEvent) -> String {
    match event {
        LogEvent::Access(e) => serde_json::to_string(e).unwrap_or_default(),
        LogEvent::Proxy(e) => serde_json::to_string(e.as_ref()).unwrap_or_default(),
        LogEvent::Audit(e) => serde_json::to_string(e).unwrap_or_default(),
        LogEvent::Shutdown => String::new(),
    }
}

/// Build the Loki push API payload from a batch of events. Events with the
/// same label set are grouped into one stream; values within each stream are
/// sorted ascending by nanosecond timestamp.
fn build_push_payload(events: &[LogEvent], static_labels: &HashMap<String, String>) -> Value {
    let mut groups: BTreeMap<BTreeMap<String, String>, Vec<(i64, String)>> = BTreeMap::new();

    for event in events {
        if matches!(event, LogEvent::Shutdown) {
            continue;
        }
        let labels = event_to_labels(event, static_labels);
        let ns = event_timestamp(event).timestamp_nanos_opt().unwrap_or(0);
        let line = event_to_line(event);
        groups.entry(labels).or_default().push((ns, line));
    }

    let streams: Vec<Value> = groups
        .into_iter()
        .map(|(labels, mut values)| {
            values.sort_by_key(|(ns, _)| *ns);
            let values_json: Vec<Value> = values
                .into_iter()
                .map(|(ns, line)| json!([ns.to_string(), line]))
                .collect();
            json!({
                "stream": labels,
                "values": values_json,
            })
        })
        .collect();

    json!({ "streams": streams })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LokiConfig;
    use crate::db::{AccessEvent, AuditEvent};
    use crate::test_utils::{make_proxy_event, mock_loki_server};
    use chrono::TimeZone;
    use std::time::Instant;

    fn make_access_event(method: &str, path: &str, status: i32) -> AccessEvent {
        AccessEvent {
            timestamp: Utc::now(),
            request_id: "req-1".to_string(),
            method: method.to_string(),
            path: path.to_string(),
            status,
            latency_ms: 10,
            client_ip: Some("127.0.0.1".to_string()),
        }
    }

    fn make_audit_event(action: &str, resource: &str) -> AuditEvent {
        AuditEvent {
            timestamp: Utc::now(),
            request_id: "req-2".to_string(),
            action: action.to_string(),
            resource: resource.to_string(),
            resource_id: "id-1".to_string(),
            detail: None,
        }
    }

    fn static_labels() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("app".to_string(), "ait".to_string());
        m
    }

    // ── Unit tests (no HTTP) ──

    #[test]
    fn test_event_to_labels_proxy() {
        let labels = static_labels();
        let event = LogEvent::Proxy(Box::new(make_proxy_event("gpt-4o", "success", 100)));
        let result = event_to_labels(&event, &labels);
        assert_eq!(result.get("app"), Some(&"ait".to_string()));
        assert_eq!(result.get("event_type"), Some(&"proxy".to_string()));
        assert_eq!(result.get("model_name"), Some(&"gpt-4o".to_string()));
        assert_eq!(result.get("status"), Some(&"success".to_string()));
        assert!(result.contains_key("is_streaming"));
        // Unbounded fields must stay out of labels (they remain in the line)
        assert!(!result.contains_key("provider_name"));
        assert!(!result.contains_key("endpoint"));
    }

    #[test]
    fn test_event_to_labels_access_and_audit() {
        let labels = static_labels();
        let access = LogEvent::Access(make_access_event("GET", "/api/providers", 200));
        let result = event_to_labels(&access, &labels);
        assert_eq!(result.get("event_type"), Some(&"access".to_string()));
        assert_eq!(result.get("method"), Some(&"GET".to_string()));
        assert!(!result.contains_key("path"), "path is unbounded, line-only");

        let audit = LogEvent::Audit(make_audit_event("create", "provider"));
        let result = event_to_labels(&audit, &labels);
        assert_eq!(result.get("event_type"), Some(&"audit".to_string()));
        assert_eq!(result.get("action"), Some(&"create".to_string()));
        assert_eq!(result.get("resource"), Some(&"provider".to_string()));
    }

    #[test]
    fn test_event_to_line_proxy_contains_fields() {
        let event = LogEvent::Proxy(Box::new(make_proxy_event("gpt-4o", "success", 100)));
        let line = event_to_line(&event);
        let parsed: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["model_name"], "gpt-4o");
        assert_eq!(parsed["status"], "success");
        assert_eq!(parsed["total_tokens"], 100);
        assert!(parsed["request_id"].is_string());
    }

    #[test]
    fn test_build_push_payload_grouping_and_sorting() {
        let labels = static_labels();
        let ts1 = Utc.timestamp_opt(1700000000, 0).unwrap();
        let ts2 = Utc.timestamp_opt(1700000001, 0).unwrap();

        // Two proxy events with same labels (same model/status) but different ts
        let mut e1 = make_proxy_event("gpt-4o", "success", 100);
        e1.timestamp = ts2; // deliberately out of order
        let mut e2 = make_proxy_event("gpt-4o", "success", 200);
        e2.timestamp = ts1;

        // One proxy event with different model (different label set)
        let mut e3 = make_proxy_event("claude-3", "success", 50);
        e3.timestamp = ts1;

        let events = vec![
            LogEvent::Proxy(Box::new(e1)),
            LogEvent::Proxy(Box::new(e2)),
            LogEvent::Proxy(Box::new(e3)),
        ];

        let payload = build_push_payload(&events, &labels);
        let streams = payload["streams"].as_array().unwrap();
        assert_eq!(streams.len(), 2, "two distinct label sets");

        // Find the gpt-4o stream and verify it has 2 values sorted ascending
        let gpt_stream = streams
            .iter()
            .find(|s| s["stream"]["model_name"] == "gpt-4o")
            .unwrap();
        let values = gpt_stream["values"].as_array().unwrap();
        assert_eq!(values.len(), 2);
        let ts0: i64 = values[0][0].as_str().unwrap().parse().unwrap();
        let ts1: i64 = values[1][0].as_str().unwrap().parse().unwrap();
        assert!(ts0 <= ts1, "values sorted ascending within stream");
    }

    #[test]
    fn test_loki_config_default_disabled() {
        let config = LokiConfig::default();
        assert!(!config.enabled);
        assert!(config.url.is_empty());
        assert_eq!(config.batch_size, 100);
        assert_eq!(config.interval_secs, 5);
    }

    #[test]
    fn test_sanitize_static_labels_drops_invalid_names() {
        let mut raw = HashMap::new();
        raw.insert("app".to_string(), "ait".to_string());
        raw.insert("bad-name".to_string(), "x".to_string());
        raw.insert("1start".to_string(), "y".to_string());
        raw.insert(String::new(), "z".to_string());

        let result = sanitize_static_labels(&raw);
        assert_eq!(result.len(), 1);
        assert_eq!(result.get("app"), Some(&"ait".to_string()));
    }

    #[test]
    fn test_is_valid_label_name() {
        assert!(is_valid_label_name("app"));
        assert!(is_valid_label_name("_private"));
        assert!(is_valid_label_name("env_2"));
        assert!(!is_valid_label_name(""));
        assert!(!is_valid_label_name("bad-name"));
        assert!(!is_valid_label_name("1start"));
        assert!(!is_valid_label_name("has space"));
        assert!(!is_valid_label_name("点"));
    }

    #[test]
    fn test_loki_sink_invalid_url_fails_fast() {
        let mut config = LokiConfig {
            enabled: true,
            ..Default::default()
        };

        config.url = "not a url".to_string();
        assert!(matches!(
            LokiSink::new(&config),
            Err(LokiInitError::InvalidUrl(_))
        ));

        // Non-http(s) schemes are rejected too
        config.url = "ftp://example.com".to_string();
        assert!(matches!(
            LokiSink::new(&config),
            Err(LokiInitError::InvalidUrl(_))
        ));
    }

    // ── Integration tests (mock HTTP server) ──

    /// Poll the captured payloads until one arrives or timeout.
    fn wait_for_payload(captured: &Arc<Mutex<Vec<Value>>>, timeout: Duration) -> Value {
        let start = Instant::now();
        loop {
            if let Some(payload) = captured.lock().unwrap().last().cloned() {
                return payload;
            }
            if start.elapsed() > timeout {
                panic!("timed out waiting for Loki push");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn test_loki_push_success() {
        let (base_url, captured) = mock_loki_server(axum::http::StatusCode::NO_CONTENT);

        let config = LokiConfig {
            enabled: true,
            url: base_url,
            labels: HashMap::new(),
            batch_size: 1,
            interval_secs: 1,
            timeout_secs: 5,
            basic_auth_user: None,
            basic_auth_password: None,
            bearer_token: None,
        };

        let sink = LokiSink::new(&config).unwrap();
        sink.send(LogEvent::Proxy(Box::new(make_proxy_event(
            "gpt-4o", "success", 100,
        ))));

        let payload = wait_for_payload(&captured, Duration::from_secs(10));
        let streams = payload["streams"].as_array().unwrap();
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0]["stream"]["event_type"], "proxy");
        assert_eq!(streams[0]["stream"]["model_name"], "gpt-4o");

        let values = streams[0]["values"].as_array().unwrap();
        assert_eq!(values.len(), 1);
        let line: Value = serde_json::from_str(values[0][1].as_str().unwrap()).unwrap();
        assert_eq!(line["model_name"], "gpt-4o");
        assert_eq!(line["total_tokens"], 100);

        sink.shutdown();
    }

    #[test]
    fn test_loki_push_failure_no_crash() {
        let (base_url, _captured) = mock_loki_server(axum::http::StatusCode::INTERNAL_SERVER_ERROR);

        let config = LokiConfig {
            enabled: true,
            url: base_url,
            labels: HashMap::new(),
            batch_size: 1,
            interval_secs: 1,
            timeout_secs: 5,
            basic_auth_user: None,
            basic_auth_password: None,
            bearer_token: None,
        };

        let sink = LokiSink::new(&config).unwrap();
        // Send multiple events — all will get 500 responses
        for _ in 0..3 {
            sink.send(LogEvent::Proxy(Box::new(make_proxy_event(
                "gpt-4o", "error", 0,
            ))));
        }

        // shutdown must not hang even after failures
        let result = std::thread::scope(|s| {
            let h = s.spawn(|| sink.shutdown());
            h.join()
        });
        assert!(result.is_ok(), "shutdown completed without hanging");
    }

    #[test]
    fn test_log_manager_dual_send() {
        let (base_url, captured) = mock_loki_server(axum::http::StatusCode::NO_CONTENT);

        let dir = tempfile::tempdir().unwrap();
        let mut config = crate::test_utils::test_config(
            dir.path().join("test.db").to_str().unwrap(),
            dir.path().join("test-logs.duckdb").to_str().unwrap(),
        );
        config.log.flush_batch = 1;
        config.log.flush_interval_secs = 1;
        config.log.loki.enabled = true;
        config.log.loki.url = base_url;
        config.log.loki.batch_size = 1;
        config.log.loki.interval_secs = 1;

        let log_manager = crate::db::LogManager::new(&config.log).unwrap();
        log_manager.log_proxy(make_proxy_event("gpt-4o", "success", 100));

        // Verify Loki received the push
        let payload = wait_for_payload(&captured, Duration::from_secs(10));
        let streams = payload["streams"].as_array().unwrap();
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0]["stream"]["event_type"], "proxy");

        log_manager.shutdown();
    }

    #[test]
    fn test_loki_disabled_no_panic() {
        let dir = tempfile::tempdir().unwrap();
        let config = crate::test_utils::test_config(
            dir.path().join("test.db").to_str().unwrap(),
            dir.path().join("test-logs.duckdb").to_str().unwrap(),
        );
        // loki is disabled by default in test_config
        let log_manager = crate::db::LogManager::new(&config.log).unwrap();
        // log_* must not panic when loki is disabled
        log_manager.log_proxy(make_proxy_event("gpt-4o", "success", 100));
        log_manager.log_access(make_access_event("GET", "/api/providers", 200));
        log_manager.log_audit(make_audit_event("create", "provider"));
        log_manager.shutdown();
    }
}
