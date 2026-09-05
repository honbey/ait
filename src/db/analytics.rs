use duckdb::Connection;
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::time::timeout;
use tracing::{error, warn};

use super::models::{
    BucketEntry, ModelDistEntry, OverviewMetrics, ProxyLogQueryParams, ProxyLogQueryResult,
    TokenDistEntry,
};

mod queries;

use queries::{
    model_dist_impl, overview_impl, query_proxy_logs_impl, requests_impl, token_dist_impl,
    tokens_impl,
};

/// Failure surfaced to callers instead of a silently empty (HTTP 200) result,
/// so the frontend can distinguish "no data" from "query failed".
#[derive(Debug, Clone, Copy)]
pub enum AnalyticsError {
    /// The query exceeded `analytics_timeout_secs`.
    Timeout,
    /// The worker is gone (channel closed) or the waiter was dropped.
    Unavailable,
    /// DuckDB rejected the query. The detail is logged by the worker and is
    /// deliberately not carried here: the admin API is unauthenticated, so
    /// driver error text must not reach a response body.
    Failed,
}

impl std::fmt::Display for AnalyticsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnalyticsError::Timeout => write!(f, "analytics query timed out"),
            AnalyticsError::Unavailable => write!(f, "analytics service unavailable"),
            AnalyticsError::Failed => write!(f, "analytics query failed"),
        }
    }
}

impl std::error::Error for AnalyticsError {}

impl AnalyticsError {
    pub fn into_response(self) -> (axum::http::StatusCode, axum::Json<crate::error::AitError>) {
        (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(crate::error::AitError {
                message: self.to_string(),
                code: 503,
                r#type: "service_unavailable".to_string(),
                detail: None,
            }),
        )
    }
}

/// Analytics workers, each with its own DuckDB connection, pulling from a
/// shared queue. Extra readers never conflict with the log worker's writes,
/// so a heavy `query_proxy_logs` no longer blocks a concurrent `/api/stats`.
const ANALYTICS_WORKERS: usize = 2;

/// LRU bound for cached prepared statements. The static aggregate queries
/// occupy three slots; the rest absorbs the `query_proxy_logs` variants,
/// whose SQL text varies with the active filters.
const STATEMENT_CACHE_CAPACITY: usize = 32;

#[derive(Clone)]
pub struct Analytics {
    tx: mpsc::Sender<AnalyticsRequest>,
    handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
    timeout: Duration,
}

/// What a worker sends back: the query result, or the reason there is none.
/// Carrying the error keeps a failed query from looking like an empty table.
type WorkerResult<T> = Result<T, AnalyticsError>;

enum AnalyticsRequest {
    Requests {
        start_ts: i64,
        end_ts: i64,
        resp: oneshot::Sender<WorkerResult<Vec<BucketEntry>>>,
    },
    Tokens {
        start_ts: i64,
        end_ts: i64,
        resp: oneshot::Sender<WorkerResult<Vec<BucketEntry>>>,
    },
    ModelDist {
        start_ts: i64,
        end_ts: i64,
        resp: oneshot::Sender<WorkerResult<Vec<ModelDistEntry>>>,
    },
    TokenDist {
        start_ts: i64,
        end_ts: i64,
        resp: oneshot::Sender<WorkerResult<Vec<TokenDistEntry>>>,
    },
    QueryProxyLogs {
        params: Box<ProxyLogQueryParams>,
        resp: oneshot::Sender<WorkerResult<ProxyLogQueryResult>>,
    },
    Overview {
        start_ts: i64,
        end_ts: i64,
        resp: oneshot::Sender<WorkerResult<OverviewMetrics>>,
    },
    Shutdown,
}

macro_rules! analytics_method {
    ($name:ident, $variant:ident, $ret:ty) => {
        pub async fn $name(&self, start_ts: i64, end_ts: i64) -> Result<$ret, AnalyticsError> {
            let (resp, rx) = oneshot::channel();
            if self
                .tx
                .send(AnalyticsRequest::$variant {
                    start_ts,
                    end_ts,
                    resp,
                })
                .is_err()
            {
                error!("[analytics] {} channel send failed", stringify!($name));
                return Err(AnalyticsError::Unavailable);
            }
            match timeout(self.timeout, rx).await {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => {
                    warn!("[analytics] {} oneshot cancelled", stringify!($name));
                    Err(AnalyticsError::Unavailable)
                }
                Err(_) => {
                    warn!("[analytics] {} timed out", stringify!($name));
                    Err(AnalyticsError::Timeout)
                }
            }
        }
    };
}

/// Run a query, containing any panic.
///
/// The worker thread owns the DuckDB connection, so a panic would unwind past
/// the receive loop and permanently fail every subsequent analytics query.
/// `None` means the query panicked; the caller then drops the response sender,
/// which the public methods report as [`AnalyticsError::Unavailable`].
fn run_guarded<T>(label: &str, query: impl FnOnce() -> T) -> Option<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(query)) {
        Ok(value) => Some(value),
        Err(_) => {
            error!("[analytics] {label} panicked");
            None
        }
    }
}

/// Reply with the query outcome.
///
/// A DuckDB failure becomes [`AnalyticsError::Failed`] so the caller can tell
/// "query broken" from "no rows": returning an empty result instead used to
/// render as a dashboard full of zeros.
fn send_result<T>(
    resp: oneshot::Sender<WorkerResult<T>>,
    label: &str,
    query: impl FnOnce() -> Result<T, duckdb::Error>,
) {
    let result = match run_guarded(label, query) {
        Some(Ok(value)) => Ok(value),
        Some(Err(e)) => {
            warn!("[analytics] {label} failed: {e}");
            Err(AnalyticsError::Failed)
        }
        // Panic: drop the sender so the caller reports the worker as gone.
        None => return,
    };
    let _ = resp.send(result);
}

impl Analytics {
    pub fn new(conn: Connection, timeout_secs: u64) -> Self {
        let (tx, rx) = mpsc::channel::<AnalyticsRequest>();
        let rx = Arc::new(Mutex::new(rx));

        // One DuckDB connection per worker. Cloning can only fail on resource
        // exhaustion; degrade to fewer workers rather than refusing to start.
        let mut connections = Vec::with_capacity(ANALYTICS_WORKERS);
        for _ in 1..ANALYTICS_WORKERS {
            match conn.try_clone() {
                Ok(clone) => connections.push(clone),
                Err(e) => {
                    warn!(
                        "[analytics] connection clone failed, degrading to {} worker(s): {e}",
                        connections.len() + 1
                    );
                    break;
                }
            }
        }
        connections.push(conn);

        let mut handles = Vec::with_capacity(connections.len());
        for worker_conn in connections {
            worker_conn.set_prepared_statement_cache_capacity(STATEMENT_CACHE_CAPACITY);
            let rx = Arc::clone(&rx);
            handles.push(std::thread::spawn(move || {
                worker_loop(worker_conn, rx);
            }));
        }

        Self {
            tx,
            handles: Arc::new(Mutex::new(handles)),
            timeout: Duration::from_secs(timeout_secs),
        }
    }
}

/// Pull requests from the shared queue until a Shutdown arrives or the
/// channel closes. The queue guard is released before the query runs, so
/// other workers can receive while this one scans.
fn worker_loop(conn: Connection, rx: Arc<Mutex<mpsc::Receiver<AnalyticsRequest>>>) {
    loop {
        let req = {
            let guard = rx.lock().unwrap_or_else(|e| e.into_inner());
            guard.recv()
        };
        let req = match req {
            Ok(req) => req,
            Err(_) => break,
        };
        match req {
            AnalyticsRequest::Requests {
                start_ts,
                end_ts,
                resp,
            } => send_result(resp, "requests", || requests_impl(&conn, start_ts, end_ts)),
            AnalyticsRequest::Tokens {
                start_ts,
                end_ts,
                resp,
            } => send_result(resp, "tokens", || tokens_impl(&conn, start_ts, end_ts)),
            AnalyticsRequest::ModelDist {
                start_ts,
                end_ts,
                resp,
            } => send_result(resp, "model_dist", || {
                model_dist_impl(&conn, start_ts, end_ts)
            }),
            AnalyticsRequest::TokenDist {
                start_ts,
                end_ts,
                resp,
            } => send_result(resp, "token_dist", || {
                token_dist_impl(&conn, start_ts, end_ts)
            }),
            AnalyticsRequest::QueryProxyLogs { params, resp } => {
                send_result(resp, "query_proxy_logs", || {
                    query_proxy_logs_impl(&conn, *params)
                })
            }
            AnalyticsRequest::Overview {
                start_ts,
                end_ts,
                resp,
            } => send_result(resp, "overview", || overview_impl(&conn, start_ts, end_ts)),
            AnalyticsRequest::Shutdown => {
                let _ = conn.execute_batch("CHECKPOINT");
                break;
            }
        }
    }
}

impl Analytics {
    analytics_method!(requests, Requests, Vec<BucketEntry>);
    analytics_method!(tokens, Tokens, Vec<BucketEntry>);
    analytics_method!(model_dist, ModelDist, Vec<ModelDistEntry>);
    analytics_method!(token_dist, TokenDist, Vec<TokenDistEntry>);

    /// All overview aggregates in one worker round trip. Individual queries
    /// still run sequentially inside the worker, but the caller pays one
    /// channel hop and one HTTP request instead of six.
    pub async fn overview(
        &self,
        start_ts: i64,
        end_ts: i64,
    ) -> Result<OverviewMetrics, AnalyticsError> {
        let (resp, rx) = oneshot::channel();
        if self
            .tx
            .send(AnalyticsRequest::Overview {
                start_ts,
                end_ts,
                resp,
            })
            .is_err()
        {
            error!("[analytics] overview channel send failed");
            return Err(AnalyticsError::Unavailable);
        }
        match timeout(self.timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                warn!("[analytics] overview oneshot cancelled");
                Err(AnalyticsError::Unavailable)
            }
            Err(_) => {
                warn!("[analytics] overview timed out");
                Err(AnalyticsError::Timeout)
            }
        }
    }

    pub async fn query_proxy_logs(
        &self,
        params: ProxyLogQueryParams,
    ) -> Result<ProxyLogQueryResult, AnalyticsError> {
        let (resp, rx) = oneshot::channel();
        if self
            .tx
            .send(AnalyticsRequest::QueryProxyLogs {
                params: Box::new(params),
                resp,
            })
            .is_err()
        {
            error!("[analytics] query_proxy_logs channel send failed");
            return Err(AnalyticsError::Unavailable);
        }
        match timeout(self.timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                warn!("[analytics] query_proxy_logs oneshot cancelled");
                Err(AnalyticsError::Unavailable)
            }
            Err(_) => {
                warn!("[analytics] query_proxy_logs timed out");
                Err(AnalyticsError::Timeout)
            }
        }
    }

    pub fn shutdown(&self) {
        // One Shutdown per worker: each worker drains queued requests until it
        // consumes one, so every thread exits exactly once.
        let worker_count = self.handles.lock().map(|h| h.len()).unwrap_or(0);
        for _ in 0..worker_count {
            let _ = self.tx.send(AnalyticsRequest::Shutdown);
        }
        if let Ok(mut guard) = self.handles.lock() {
            for handle in guard.drain(..) {
                let _ = handle.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::logger::create_schema;
    use chrono::{DateTime, Utc};
    use duckdb::params;

    fn setup() -> (tempfile::TempDir, Connection, Analytics) {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("analytics.duckdb");
        let conn = Connection::open(&path).unwrap();
        create_schema(&conn).unwrap();
        let analytics = Analytics::new(conn.try_clone().unwrap(), 5);
        (dir, conn, analytics)
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_proxy(
        conn: &Connection,
        ts: i64,
        model: &str,
        status: &str,
        prompt: i64,
        completion: i64,
        cached: i64,
    ) {
        let naive = DateTime::from_timestamp(ts, 0).unwrap().naive_utc();
        conn.execute(
            "INSERT INTO proxy_log (timestamp, request_id, api_key_name, model_name,
             provider_name, prompt_tokens, completion_tokens, total_tokens, cached_tokens,
             latency_ms, status, endpoint, is_streaming, upstream_model, provider_type, client_ip)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
            params![
                naive,
                "req-1",
                "test-key",
                model,
                "test-provider",
                prompt,
                completion,
                prompt + completion,
                cached,
                50,
                status,
                "/v1/chat/completions",
                false,
                model,
                "openai_compat",
                "127.0.0.1"
            ],
        )
        .unwrap();
    }

    fn hour_floor() -> i64 {
        Utc::now().timestamp() / 3600 * 3600
    }

    #[tokio::test]
    async fn overview_totals_filter_by_range() {
        let (_dir, conn, analytics) = setup();
        insert_proxy(&conn, hour_floor() + 100, "gpt-4", "success", 10, 20, 5);
        insert_proxy(&conn, hour_floor() - 3000, "llama", "success", 30, 40, 0);

        let wide = analytics
            .overview(hour_floor() - 7200, hour_floor() + 7200)
            .await
            .unwrap();
        assert_eq!(wide.total_requests, 2);
        assert_eq!(wide.total_tokens, 100);

        // Narrow range excludes the second event.
        let narrow = analytics
            .overview(hour_floor() - 1800, hour_floor() + 1800)
            .await
            .unwrap();
        assert_eq!(narrow.total_requests, 1);
    }

    #[tokio::test]
    async fn empty_range_returns_zero() {
        let (_dir, _conn, analytics) = setup();
        let m = analytics.overview(0, 1).await.unwrap();
        assert_eq!(m.total_requests, 0);
        assert_eq!(m.total_tokens, 0);
        assert!(analytics.requests(0, 1).await.unwrap().is_empty());
        assert!(analytics.model_dist(0, 1).await.unwrap().is_empty());
        let dist = analytics.token_dist(0, 1).await.unwrap();
        assert!(dist.iter().all(|e| e.count == 0));
    }

    #[tokio::test]
    async fn failing_query_surfaces_error_instead_of_empty_result() {
        let (_dir, conn, analytics) = setup();
        insert_proxy(&conn, hour_floor() + 10, "gpt-4", "success", 1, 1, 0);
        // Take the table away so every query fails. Callers must see that
        // rather than a successful response full of zeros.
        conn.execute_batch("DROP TABLE proxy_log").unwrap();

        assert!(
            matches!(
                analytics.overview(0, hour_floor() + 3600).await,
                Err(AnalyticsError::Failed)
            ),
            "overview must report the failure"
        );
        assert!(
            matches!(
                analytics.requests(0, hour_floor() + 3600).await,
                Err(AnalyticsError::Failed)
            ),
            "requests must report the failure"
        );
        assert!(
            matches!(
                analytics
                    .query_proxy_logs(ProxyLogQueryParams::default())
                    .await,
                Err(AnalyticsError::Failed)
            ),
            "query_proxy_logs must report the failure"
        );
    }

    #[tokio::test]
    async fn requests_buckets_by_hour() {
        let (_dir, conn, analytics) = setup();
        let h = hour_floor();
        insert_proxy(&conn, h + 120, "gpt-4", "success", 1, 1, 0);
        insert_proxy(&conn, h + 1800, "llama", "success", 1, 1, 0);
        insert_proxy(&conn, h - 3600 + 900, "gpt-4", "success", 1, 1, 0);

        let buckets = analytics.requests(h - 7200, h + 7200).await.unwrap();
        assert_eq!(buckets.len(), 2);
        assert_eq!(buckets[0].timestamp, h - 3600);
        assert_eq!(buckets[0].count, 1);
        assert_eq!(buckets[1].timestamp, h);
        assert_eq!(buckets[1].count, 2);

        let tokens = analytics.tokens(h - 7200, h + 7200).await.unwrap();
        assert_eq!(tokens[1].count, 4);
    }

    #[tokio::test]
    async fn model_dist_groups_and_orders_desc() {
        let (_dir, conn, analytics) = setup();
        let h = hour_floor();
        insert_proxy(&conn, h + 10, "gpt-4", "success", 1, 1, 0);
        insert_proxy(&conn, h + 20, "gpt-4", "success", 1, 1, 0);
        insert_proxy(&conn, h + 30, "llama", "success", 1, 1, 0);

        let dist = analytics.model_dist(h - 3600, h + 3600).await.unwrap();
        assert_eq!(dist.len(), 2);
        assert_eq!(dist[0].model, "gpt-4");
        assert_eq!(dist[0].count, 2);
        assert_eq!(dist[1].model, "llama");
        assert_eq!(dist[1].count, 1);
    }

    #[tokio::test]
    async fn token_dist_splits_categories() {
        let (_dir, conn, analytics) = setup();
        let h = hour_floor();
        insert_proxy(&conn, h + 10, "gpt-4", "success", 100, 30, 20);

        let dist = analytics.token_dist(h - 3600, h + 3600).await.unwrap();
        let get = |cat: &str| {
            dist.iter()
                .find(|e| e.category == cat)
                .map(|e| e.count)
                .unwrap_or(0)
        };
        assert_eq!(get("uncached_input"), 80);
        assert_eq!(get("cached_input"), 20);
        assert_eq!(get("output"), 30);

        // cached > prompt is clamped to prompt. Aggregation sums columns
        // first, then splits: prompt=150, completion=35, cached=120.
        insert_proxy(&conn, h + 20, "llama", "success", 50, 5, 100);
        let dist = analytics.token_dist(h - 3600, h + 3600).await.unwrap();
        let get = |cat: &str| {
            dist.iter()
                .find(|e| e.category == cat)
                .map(|e| e.count)
                .unwrap_or(0)
        };
        assert_eq!(get("uncached_input"), 30);
        assert_eq!(get("cached_input"), 120);
        assert_eq!(get("output"), 35);
    }

    #[tokio::test]
    async fn overview_returns_all_aggregates() {
        let (_dir, conn, analytics) = setup();
        let h = hour_floor();
        insert_proxy(&conn, h + 10, "gpt-4", "success", 100, 30, 20);
        insert_proxy(&conn, h + 20, "llama", "success", 10, 5, 0);

        let m = analytics.overview(h - 3600, h + 3600).await.unwrap();
        assert_eq!(m.total_requests, 2);
        assert_eq!(m.total_tokens, 145);
        assert_eq!(m.request_buckets.len(), 1);
        assert_eq!(m.request_buckets[0].count, 2);
        assert_eq!(m.token_buckets.len(), 1);
        assert_eq!(m.token_buckets[0].count, 145);
        assert_eq!(m.model_dist.len(), 2);
        let get = |cat: &str| {
            m.token_dist
                .iter()
                .find(|e| e.category == cat)
                .map(|e| e.count)
                .unwrap_or(0)
        };
        assert_eq!(get("uncached_input"), 90);
        assert_eq!(get("cached_input"), 20);
        assert_eq!(get("output"), 35);
    }

    #[tokio::test]
    async fn query_proxy_logs_filters_and_paginates() {
        let (_dir, conn, analytics) = setup();
        let h = hour_floor();
        for i in 0..5 {
            let (model, status) = if i % 2 == 0 {
                ("gpt-4", "success")
            } else {
                ("llama", "error")
            };
            insert_proxy(&conn, h + i * 60, model, status, 10, 10, 0);
        }

        let params = ProxyLogQueryParams {
            page: 1,
            per_page: 10,
            start_ts: Some(h - 3600),
            end_ts: Some(h + 3600),
            model_name: Some("gpt-4".to_string()),
            ..Default::default()
        };
        let result = analytics.query_proxy_logs(params).await.unwrap();
        assert_eq!(result.total, 3);
        assert_eq!(result.items.len(), 3);
        assert!(result.items.iter().all(|e| e.model_name == "gpt-4"));

        // Pagination: page 2 of 2 rows per page (5 rows total).
        let params = ProxyLogQueryParams {
            page: 2,
            per_page: 2,
            start_ts: Some(h - 3600),
            end_ts: Some(h + 3600),
            ..Default::default()
        };
        let result = analytics.query_proxy_logs(params).await.unwrap();
        assert_eq!(result.total, 5);
        assert_eq!(result.items.len(), 2);
        // Ordered by timestamp DESC: items are at h+240, h+180.
        assert!(result.items[0].timestamp > result.items[1].timestamp);

        // Combined filters.
        let params = ProxyLogQueryParams {
            page: 1,
            per_page: 10,
            start_ts: Some(h - 3600),
            end_ts: Some(h + 3600),
            model_name: Some("llama".to_string()),
            status: Some("error".to_string()),
            ..Default::default()
        };
        let result = analytics.query_proxy_logs(params).await.unwrap();
        assert_eq!(result.total, 2);
    }
}
