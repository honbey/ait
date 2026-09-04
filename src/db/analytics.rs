use chrono::DateTime;
use duckdb::{Connection, params, params_from_iter};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::time::timeout;
use tracing::{error, warn};

use super::models::{
    BucketEntry, ModelDistEntry, OverviewMetrics, ProxyLogEntryResponse, ProxyLogQueryParams,
    ProxyLogQueryResult, TokenDistEntry,
};

/// Failure surfaced to callers instead of a silently empty (HTTP 200) result,
/// so the frontend can distinguish "no data" from "query failed".
#[derive(Debug, Clone, Copy)]
pub enum AnalyticsError {
    /// The query exceeded `analytics_timeout_secs`.
    Timeout,
    /// The worker is gone (channel closed) or the waiter was dropped.
    Unavailable,
}

impl std::fmt::Display for AnalyticsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnalyticsError::Timeout => write!(f, "analytics query timed out"),
            AnalyticsError::Unavailable => write!(f, "analytics service unavailable"),
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

#[derive(Clone)]
pub struct Analytics {
    tx: mpsc::Sender<AnalyticsRequest>,
    handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    timeout: Duration,
}

enum AnalyticsRequest {
    Requests {
        start_ts: i64,
        end_ts: i64,
        resp: oneshot::Sender<Vec<BucketEntry>>,
    },
    Tokens {
        start_ts: i64,
        end_ts: i64,
        resp: oneshot::Sender<Vec<BucketEntry>>,
    },
    ModelDist {
        start_ts: i64,
        end_ts: i64,
        resp: oneshot::Sender<Vec<ModelDistEntry>>,
    },
    TokenDist {
        start_ts: i64,
        end_ts: i64,
        resp: oneshot::Sender<Vec<TokenDistEntry>>,
    },
    QueryProxyLogs {
        params: Box<ProxyLogQueryParams>,
        resp: oneshot::Sender<ProxyLogQueryResult>,
    },
    Overview {
        start_ts: i64,
        end_ts: i64,
        resp: oneshot::Sender<OverviewMetrics>,
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
                Ok(inner) => inner.map_err(|_| {
                    warn!("[analytics] {} oneshot cancelled", stringify!($name));
                    AnalyticsError::Unavailable
                }),
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
/// the receive loop, drop the receiver, and permanently fail every subsequent
/// analytics query with an empty result and no visible failure. Dropping the
/// `resp` sender makes the caller's `await` yield `Err`, which the public
/// methods already map to `Default`.
fn run_guarded<T>(label: &str, query: impl FnOnce() -> T) -> Option<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(query)) {
        Ok(value) => Some(value),
        Err(_) => {
            error!("[analytics] {label} panicked; returning empty result");
            None
        }
    }
}

impl Analytics {
    pub fn new(conn: Connection, timeout_secs: u64) -> Self {
        let (tx, rx) = mpsc::channel::<AnalyticsRequest>();
        let handle = std::thread::spawn(move || {
            for req in rx {
                match req {
                    AnalyticsRequest::Requests {
                        start_ts,
                        end_ts,
                        resp,
                    } => {
                        if let Some(value) =
                            run_guarded("requests", || requests_impl(&conn, start_ts, end_ts))
                        {
                            let _ = resp.send(value);
                        }
                    }
                    AnalyticsRequest::Tokens {
                        start_ts,
                        end_ts,
                        resp,
                    } => {
                        if let Some(value) =
                            run_guarded("tokens", || tokens_impl(&conn, start_ts, end_ts))
                        {
                            let _ = resp.send(value);
                        }
                    }
                    AnalyticsRequest::ModelDist {
                        start_ts,
                        end_ts,
                        resp,
                    } => {
                        if let Some(value) =
                            run_guarded("model_dist", || model_dist_impl(&conn, start_ts, end_ts))
                        {
                            let _ = resp.send(value);
                        }
                    }
                    AnalyticsRequest::TokenDist {
                        start_ts,
                        end_ts,
                        resp,
                    } => {
                        if let Some(value) =
                            run_guarded("token_dist", || token_dist_impl(&conn, start_ts, end_ts))
                        {
                            let _ = resp.send(value);
                        }
                    }
                    AnalyticsRequest::QueryProxyLogs { params, resp } => {
                        if let Some(value) = run_guarded("query_proxy_logs", || {
                            query_proxy_logs_impl(&conn, *params)
                        }) {
                            let _ = resp.send(value);
                        }
                    }
                    AnalyticsRequest::Overview {
                        start_ts,
                        end_ts,
                        resp,
                    } => {
                        if let Some(value) =
                            run_guarded("overview", || overview_impl(&conn, start_ts, end_ts))
                        {
                            let _ = resp.send(value);
                        }
                    }
                    AnalyticsRequest::Shutdown => {
                        let _ = conn.execute_batch("CHECKPOINT");
                        break;
                    }
                }
            }
        });
        Self {
            tx,
            handle: Arc::new(Mutex::new(Some(handle))),
            timeout: Duration::from_secs(timeout_secs),
        }
    }

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
            Ok(inner) => inner.map_err(|_| {
                warn!("[analytics] overview oneshot cancelled");
                AnalyticsError::Unavailable
            }),
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
            Ok(inner) => inner.map_err(|_| {
                warn!("[analytics] query_proxy_logs oneshot cancelled");
                AnalyticsError::Unavailable
            }),
            Err(_) => {
                warn!("[analytics] query_proxy_logs timed out");
                Err(AnalyticsError::Timeout)
            }
        }
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(AnalyticsRequest::Shutdown);
        if let Ok(mut guard) = self.handle.lock()
            && let Some(handle) = guard.take()
        {
            let _ = handle.join();
        }
    }
}

fn ts_range(start_ts: i64, end_ts: i64) -> (chrono::NaiveDateTime, chrono::NaiveDateTime) {
    let start = DateTime::from_timestamp(start_ts, 0)
        .unwrap_or(DateTime::UNIX_EPOCH)
        .naive_utc();
    let end = DateTime::from_timestamp(end_ts, 0)
        .unwrap_or(DateTime::UNIX_EPOCH)
        .naive_utc();
    (start, end)
}

/// Totals for a range in one scan: request count, total tokens, and the three
/// token-kind sums that `token_dist_from_sums` splits into categories.
/// `overview_impl` previously issued three separate scans for these.
fn totals_impl(conn: &Connection, start_ts: i64, end_ts: i64) -> (u64, u64, i64, i64, i64) {
    let (start, end) = ts_range(start_ts, end_ts);
    let default = (0u64, 0u64, 0i64, 0i64, 0i64);
    match conn.prepare_cached(
        "SELECT COUNT(*), \
                COALESCE(SUM(total_tokens), 0), \
                COALESCE(SUM(prompt_tokens), 0), \
                COALESCE(SUM(completion_tokens), 0), \
                COALESCE(SUM(cached_tokens), 0) \
         FROM proxy_log WHERE timestamp >= ?1 AND timestamp < ?2",
    ) {
        Ok(mut stmt) => stmt
            .query_row(params![start, end], |row| {
                Ok((
                    row.get::<_, u64>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .unwrap_or_else(|e| {
                warn!("[analytics] totals query failed: {e}");
                default
            }),
        Err(e) => {
            warn!("[analytics] totals prepare failed: {e}");
            default
        }
    }
}

/// Hourly request and token buckets in one scan; shared by the standalone
/// endpoints and by `overview_impl`.
fn hourly_buckets_impl(
    conn: &Connection,
    start_ts: i64,
    end_ts: i64,
) -> (Vec<BucketEntry>, Vec<BucketEntry>) {
    let (start, end) = ts_range(start_ts, end_ts);
    let mut stmt = match conn.prepare_cached(
        "SELECT epoch(DATE_TRUNC('hour', timestamp)) AS bucket_ts, \
                COUNT(*), \
                COALESCE(SUM(total_tokens), 0) \
         FROM proxy_log WHERE timestamp >= ?1 AND timestamp < ?2 \
         GROUP BY bucket_ts ORDER BY bucket_ts",
    ) {
        Ok(s) => s,
        Err(e) => {
            warn!("[analytics] hourly buckets prepare failed: {e}");
            return (Vec::new(), Vec::new());
        }
    };
    let rows = match stmt.query_map(params![start, end], |row| {
        Ok((
            row.get::<_, f64>(0)? as i64,
            row.get::<_, i64>(1)? as u64,
            row.get::<_, i64>(2)? as u64,
        ))
    }) {
        Ok(r) => r,
        Err(e) => {
            warn!("[analytics] hourly buckets query failed: {e}");
            return (Vec::new(), Vec::new());
        }
    };
    let mut requests = Vec::new();
    let mut tokens = Vec::new();
    for r in rows.flatten() {
        requests.push(BucketEntry {
            timestamp: r.0,
            count: r.1,
        });
        tokens.push(BucketEntry {
            timestamp: r.0,
            count: r.2,
        });
    }
    (requests, tokens)
}

fn requests_impl(conn: &Connection, start_ts: i64, end_ts: i64) -> Vec<BucketEntry> {
    hourly_buckets_impl(conn, start_ts, end_ts).0
}

fn tokens_impl(conn: &Connection, start_ts: i64, end_ts: i64) -> Vec<BucketEntry> {
    hourly_buckets_impl(conn, start_ts, end_ts).1
}

fn model_dist_impl(conn: &Connection, start_ts: i64, end_ts: i64) -> Vec<ModelDistEntry> {
    let (start, end) = ts_range(start_ts, end_ts);
    let mut stmt = match conn.prepare_cached(
        "SELECT model_name, COUNT(*) AS count \
         FROM proxy_log WHERE timestamp >= ?1 AND timestamp < ?2 \
         GROUP BY model_name ORDER BY count DESC",
    ) {
        Ok(s) => s,
        Err(e) => {
            warn!("[analytics] model_dist prepare failed: {e}");
            return Vec::new();
        }
    };
    let rows = match stmt.query_map(params![start, end], |row| {
        Ok(ModelDistEntry {
            model: row.get(0)?,
            count: row.get::<_, i64>(1)? as u64,
        })
    }) {
        Ok(r) => r,
        Err(e) => {
            warn!("[analytics] model_dist query failed: {e}");
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for r in rows.flatten() {
        out.push(r);
    }
    out
}

/// Split token sums into the three reported categories. Pure so
/// `overview_impl` can reuse the sums `totals_impl` already fetched.
fn token_dist_from_sums(prompt: i64, completion: i64, cached: i64) -> Vec<TokenDistEntry> {
    let prompt = prompt.max(0) as u64;
    let completion = completion.max(0) as u64;
    let cached = cached.max(0) as u64;
    let uncached_input = prompt - cached.min(prompt);

    vec![
        TokenDistEntry {
            category: "uncached_input".into(),
            count: uncached_input,
        },
        TokenDistEntry {
            category: "cached_input".into(),
            count: cached.min(prompt),
        },
        TokenDistEntry {
            category: "output".into(),
            count: completion,
        },
    ]
}

fn token_dist_impl(conn: &Connection, start_ts: i64, end_ts: i64) -> Vec<TokenDistEntry> {
    let (_, _, prompt, completion, cached) = totals_impl(conn, start_ts, end_ts);
    token_dist_from_sums(prompt, completion, cached)
}

fn overview_impl(conn: &Connection, start_ts: i64, end_ts: i64) -> OverviewMetrics {
    // Three scans cover all six aggregates the dashboard needs.
    let (total_requests, total_tokens, prompt, completion, cached) =
        totals_impl(conn, start_ts, end_ts);
    let (request_buckets, token_buckets) = hourly_buckets_impl(conn, start_ts, end_ts);
    OverviewMetrics {
        total_requests,
        total_tokens,
        request_buckets,
        token_buckets,
        model_dist: model_dist_impl(conn, start_ts, end_ts),
        token_dist: token_dist_from_sums(prompt, completion, cached),
    }
}

fn query_proxy_logs_impl(conn: &Connection, params: ProxyLogQueryParams) -> ProxyLogQueryResult {
    use duckdb::types::ToSql;

    let mut conditions: Vec<String> = Vec::new();
    let mut values: Vec<Box<dyn ToSql>> = Vec::new();

    let push_naive = |values: &mut Vec<Box<dyn ToSql>>, ts: i64| {
        let naive = DateTime::from_timestamp(ts, 0)
            .unwrap_or(DateTime::UNIX_EPOCH)
            .naive_utc();
        values.push(Box::new(naive));
    };

    if let Some(start) = params.start_ts {
        conditions.push("timestamp >= ?".into());
        push_naive(&mut values, start);
    }
    if let Some(end) = params.end_ts {
        conditions.push("timestamp < ?".into());
        push_naive(&mut values, end);
    }
    if let Some(ref model_name) = params.model_name
        && !model_name.is_empty()
    {
        conditions.push("model_name = ?".into());
        values.push(Box::new(model_name.clone()));
    }
    if let Some(ref provider_name) = params.provider_name
        && !provider_name.is_empty()
    {
        conditions.push("provider_name = ?".into());
        values.push(Box::new(provider_name.clone()));
    }
    if let Some(ref status) = params.status
        && !status.is_empty()
    {
        conditions.push("status = ?".into());
        values.push(Box::new(status.clone()));
    }
    if let Some(ref api_key_name) = params.api_key_name
        && !api_key_name.is_empty()
    {
        conditions.push("api_key_name = ?".into());
        values.push(Box::new(api_key_name.clone()));
    }
    if let Some(ref endpoint) = params.endpoint
        && !endpoint.is_empty()
    {
        conditions.push("endpoint = ?".into());
        values.push(Box::new(endpoint.clone()));
    }
    if let Some(is_streaming) = params.is_streaming {
        conditions.push("is_streaming = ?".into());
        values.push(Box::new(is_streaming));
    }
    if let Some(ref upstream_model) = params.upstream_model
        && !upstream_model.is_empty()
    {
        conditions.push("upstream_model = ?".into());
        values.push(Box::new(upstream_model.clone()));
    }
    if let Some(ref provider_type) = params.provider_type
        && !provider_type.is_empty()
    {
        conditions.push("provider_type = ?".into());
        values.push(Box::new(provider_type.clone()));
    }
    if let Some(ref client_ip) = params.client_ip
        && !client_ip.is_empty()
    {
        conditions.push("client_ip = ?".into());
        values.push(Box::new(client_ip.clone()));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let values_ref: Vec<&dyn ToSql> = values.iter().map(|v| v.as_ref()).collect();

    // Count query
    let count_sql = format!("SELECT COUNT(*) FROM proxy_log{where_clause}");
    let total: u64 = match conn.query_row(&count_sql, params_from_iter(values_ref.clone()), |row| {
        row.get::<_, u64>(0)
    }) {
        Ok(n) => n,
        Err(e) => {
            warn!("[analytics] query_proxy_logs count failed: {e}");
            0
        }
    };

    // Data query. Both operands are user-controlled, so saturate instead of
    // overflowing: an arithmetic panic here would kill the analytics worker.
    let offset = params
        .page
        .saturating_sub(1)
        .saturating_mul(params.per_page);
    let data_sql = format!(
        "SELECT timestamp, api_key_name, model_name, provider_name, \
         prompt_tokens, completion_tokens, total_tokens, cached_tokens, latency_ms, status, \
         endpoint, is_streaming, time_to_first_token_ms, upstream_model, provider_type, \
         response_body_size, error_message, client_ip, request_id \
         FROM proxy_log{where_clause} ORDER BY timestamp DESC LIMIT ? OFFSET ?"
    );

    let mut data_params: Vec<&dyn ToSql> = values_ref;
    let limit_val: i64 = params.per_page as i64;
    let offset_val: i64 = offset as i64;
    data_params.push(&limit_val);
    data_params.push(&offset_val);

    let items: Vec<ProxyLogEntryResponse> = match conn.prepare(&data_sql).and_then(|mut stmt| {
        let rows = stmt.query_map(params_from_iter(data_params), |row| {
            Ok(ProxyLogEntryResponse {
                timestamp: row
                    .get::<_, chrono::NaiveDateTime>(0)?
                    .and_utc()
                    .timestamp(),
                api_key_name: row.get(1)?,
                model_name: row.get(2)?,
                provider_name: row.get(3)?,
                prompt_tokens: row.get(4)?,
                completion_tokens: row.get(5)?,
                total_tokens: row.get(6)?,
                cached_tokens: row.get(7)?,
                latency_ms: row.get(8)?,
                status: row.get(9)?,
                endpoint: row.get(10)?,
                is_streaming: row.get(11)?,
                time_to_first_token_ms: row.get(12)?,
                upstream_model: row.get(13)?,
                provider_type: row.get(14)?,
                response_body_size: row.get(15)?,
                error_message: row.get(16)?,
                client_ip: row.get(17)?,
                request_id: row.get(18)?,
            })
        })?;
        let mut out = Vec::new();
        for item in rows.flatten() {
            out.push(item);
        }
        Ok(out)
    }) {
        Ok(items) => items,
        Err(e) => {
            warn!("[analytics] query_proxy_logs data query failed: {e}");
            Vec::new()
        }
    };

    ProxyLogQueryResult { items, total }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::logger::create_schema;
    use chrono::{DateTime, Utc};

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
