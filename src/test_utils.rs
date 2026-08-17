use crate::app::AppState;
use crate::config::{
    AuthConfig, ConfigApp, DatabaseConfig, DlpConfig, LogConfig, ProxyConfig, SecurityConfig,
    ServerConfig,
};
use crate::db::{
    AccessEvent, AuditEvent, Database, LogManager, Model, Provider, ProviderType, ProxyEvent,
};
use crate::dlp::DlpScanner;
use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use chrono::Utc;
use dashmap::DashMap;
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

pub fn create_test_db() -> (Database, TempDir) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.db");
    let db = Database::new(path.to_str().unwrap()).unwrap();
    (db, dir)
}

pub fn create_test_provider(id: &str, provider_type: ProviderType, base_url: &str) -> Provider {
    Provider {
        id: id.to_string(),
        name: id.to_string(),
        provider_type,
        base_url: base_url.to_string(),
        api_key: None,
        enabled: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

pub fn create_test_model(name: &str, provider_id: &str) -> Model {
    Model {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.to_string(),
        provider_id: provider_id.to_string(),
        upstream_model: name.to_string(),
        enabled: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

// ── HTTP integration test helpers ──

/// Config with temp DB/log paths and SSRF allowed for localhost, so tests can
/// create providers with a `http://127.0.0.1` base URL without real DNS.
pub(crate) fn test_config(db_path: &str, log_path: &str) -> ConfigApp {
    ConfigApp {
        server: ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 8000,
            health_detail: false,
            cache_cleanup_interval_secs: 300,
            cache_max_entries: 1000,
            graceful_timeout_secs: 10,
            trusted_proxies: vec!["127.0.0.1".parse().unwrap(), "::1".parse().unwrap()],
        },
        auth: AuthConfig { enabled: true },
        database: DatabaseConfig {
            path: db_path.to_string(),
        },
        log: LogConfig {
            path: log_path.to_string(),
            retention_days: 30,
            flush_interval_secs: 10,
            flush_batch: 100,
            channel_cap: 10000,
            retention_every: 100,
            level: "info".to_string(),
            axum: "info".to_string(),
            tower_http_trace: "info".to_string(),
            analytics_timeout_secs: 10,
        },
        proxy: ProxyConfig {
            timeout_secs: 300,
            stream: true,
            sse_idle_timeout_secs: 60,
            connect_timeout_secs: 30,
            max_response_body_bytes: 8 * 1024 * 1024,
            max_request_body_bytes: 8 * 1024 * 1024,
        },
        security: SecurityConfig {
            ssrf_allowed_cidrs: vec!["127.0.0.1/8".to_string()],
            cors_allowed_origins: vec![],
            cors_allow_credentials: false,
            dlp: DlpConfig::default(),
        },
    }
}

/// Variant of `test_config` that flushes every log event immediately, so tests
/// can read back written events without waiting for the flush interval.
/// Retention cleanup is disabled (`retention_every` maxed) to keep tests
/// deterministic.
pub(crate) fn test_config_fast_logs(db_path: &str, log_path: &str) -> ConfigApp {
    let mut config = test_config(db_path, log_path);
    config.log.flush_batch = 1;
    config.log.flush_interval_secs = 1;
    config.log.retention_every = u64::MAX;
    config
}

fn build_state(config: ConfigApp, dir: TempDir) -> (AppState, TempDir) {
    let db = Arc::new(Database::new(&config.database.path).unwrap());
    let log_manager = LogManager::new(&config.log).unwrap();
    let dlp = DlpScanner::new(&config.security.dlp);
    let state = AppState {
        config,
        db,
        http_client: reqwest::Client::new(),
        log_manager,
        start_time: Utc::now(),
        shutdown_token: CancellationToken::new(),
        api_key_cache: Arc::new(DashMap::new()),
        model_cache: Arc::new(DashMap::new()),
        provider_cache: Arc::new(DashMap::new()),
        ssrf_dns_cache: Arc::new(DashMap::new()),
        dlp,
    };
    (state, dir)
}

/// Build an AppState without spawning the background cleanup tasks, so tests
/// leave no running tasks behind. Returns the state together with the TempDir
/// that keeps the DB files alive for the duration of the test.
pub fn create_test_state() -> (AppState, TempDir) {
    let dir = TempDir::new().unwrap();
    let config = test_config(
        dir.path().join("test.db").to_str().unwrap(),
        dir.path().join("test-logs.duckdb").to_str().unwrap(),
    );
    build_state(config, dir)
}

/// Like `create_test_state`, but with the fast-flush log config for tests that
/// assert on written log events.
pub fn create_test_state_fast_logs() -> (AppState, TempDir) {
    let dir = TempDir::new().unwrap();
    let config = test_config_fast_logs(
        dir.path().join("test.db").to_str().unwrap(),
        dir.path().join("test-logs.duckdb").to_str().unwrap(),
    );
    build_state(config, dir)
}

/// Build a test AppState with the DLP scanner enabled for the given literal
/// values, so handlers exercise the sensitive-data block path.
pub fn create_test_state_dlp(values: &[&str]) -> (AppState, TempDir) {
    let dir = TempDir::new().unwrap();
    let mut config = test_config(
        dir.path().join("test.db").to_str().unwrap(),
        dir.path().join("test-logs.duckdb").to_str().unwrap(),
    );
    config.security.dlp.enabled = true;
    config.security.dlp.sensitive_values = values.iter().map(|s| s.to_string()).collect();
    build_state(config, dir)
}

// ── Log event factory helpers ──

pub fn make_proxy_event(model: &str, status: &str, total_tokens: i64) -> ProxyEvent {
    ProxyEvent {
        timestamp: Utc::now(),
        request_id: uuid::Uuid::new_v4().to_string(),
        api_key_name: Some("test-key".to_string()),
        model_name: model.to_string(),
        provider_name: "test-provider".to_string(),
        prompt_tokens: Some(10),
        completion_tokens: Some(20),
        total_tokens: Some(total_tokens),
        cached_tokens: Some(5),
        latency_ms: 100,
        status: status.to_string(),
        endpoint: "/v1/chat/completions".to_string(),
        is_streaming: false,
        time_to_first_token_ms: None,
        upstream_model: model.to_string(),
        provider_type: "openai_compat".to_string(),
        response_body_size: Some(1024),
        error_message: None,
        client_ip: Some("127.0.0.1".to_string()),
    }
}

pub fn make_audit_event(action: &str) -> AuditEvent {
    AuditEvent {
        timestamp: Utc::now(),
        request_id: uuid::Uuid::new_v4().to_string(),
        action: action.to_string(),
        resource: "api_key".to_string(),
        resource_id: "key-1".to_string(),
        detail: None,
    }
}

pub fn make_access_event(path: &str, status: i32) -> AccessEvent {
    AccessEvent {
        timestamp: Utc::now(),
        request_id: uuid::Uuid::new_v4().to_string(),
        method: "GET".to_string(),
        path: path.to_string(),
        status,
        latency_ms: 42,
        client_ip: Some("127.0.0.1".to_string()),
    }
}

/// The full router as built by `main`, middleware included.
pub fn test_router(state: AppState) -> Router {
    crate::build_app(state)
}

pub struct TestResponse {
    pub status: StatusCode,
    pub json: Value,
    pub headers: HeaderMap,
}

/// Send a request through the router. `cookie` is the raw session key, sent as
/// the `session_key` cookie; `bearer` is a raw API key sent as a Bearer token.
/// A fake client IP is always injected because the login rate limiter and
/// client IP extraction require `ConnectInfo`.
pub async fn send_request(
    router: &Router,
    method: Method,
    uri: &str,
    cookie: Option<&str>,
    bearer: Option<&str>,
    body: Option<Value>,
) -> TestResponse {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, format!("session_key={cookie}"));
    }
    if let Some(bearer) = bearer {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {bearer}"));
    }
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let mut request = builder.body(Body::from(body)).unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 0))));
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    TestResponse {
        status,
        json,
        headers,
    }
}

/// Login and return the raw session key from the `Set-Cookie` header.
/// Run a blocking closure on a separate thread and fail the test if it does
/// not complete within `timeout`. A timeout means the closure deadlocked on a
/// non-reentrant lock (e.g. DashMap with parking_lot); a disconnect means the
/// closure panicked. The thread is deliberately not joined: a stuck thread is
/// reclaimed when the test process exits instead of hanging the test runner.
pub fn assert_no_deadlock<T: Send + 'static>(
    timeout: std::time::Duration,
    f: impl FnOnce() -> T + Send + 'static,
) -> T {
    let (tx, rx) = std::sync::mpsc::channel::<T>();
    std::thread::spawn(move || {
        let result = f();
        let _ = tx.send(result);
    });
    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("potential deadlock: operation did not complete within {timeout:?}")
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            panic!("operation panicked before sending a result")
        }
    }
}
