use chrono::DateTime;
use duckdb::{Connection, params, params_from_iter};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::time::timeout;
use tracing::{error, warn};

use super::models::{
    BucketEntry, ModelDistEntry, ProxyLogEntryResponse, ProxyLogQueryParams, ProxyLogQueryResult,
    TokenDistEntry,
};

#[derive(Clone)]
pub struct Analytics {
    tx: mpsc::Sender<AnalyticsRequest>,
    handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    timeout: Duration,
}

enum AnalyticsRequest {
    TotalRequests {
        start_ts: i64,
        end_ts: i64,
        resp: oneshot::Sender<u64>,
    },
    TotalTokens {
        start_ts: i64,
        end_ts: i64,
        resp: oneshot::Sender<u64>,
    },
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
    Shutdown,
}

macro_rules! analytics_method {
    ($name:ident, $variant:ident, $ret:ty) => {
        pub async fn $name(&self, start_ts: i64, end_ts: i64) -> $ret {
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
                return Default::default();
            }
            match timeout(self.timeout, rx).await {
                Ok(inner) => inner.unwrap_or_else(|_| {
                    warn!("[analytics] {} oneshot cancelled", stringify!($name));
                    Default::default()
                }),
                Err(_) => {
                    warn!("[analytics] {} timed out", stringify!($name));
                    Default::default()
                }
            }
        }
    };
}

impl Analytics {
    pub fn new(conn: Connection, timeout_secs: u64) -> Self {
        let (tx, rx) = mpsc::channel::<AnalyticsRequest>();
        let handle = std::thread::spawn(move || {
            for req in rx {
                match req {
                    AnalyticsRequest::TotalRequests {
                        start_ts,
                        end_ts,
                        resp,
                    } => {
                        let _ = resp.send(total_requests_impl(&conn, start_ts, end_ts));
                    }
                    AnalyticsRequest::TotalTokens {
                        start_ts,
                        end_ts,
                        resp,
                    } => {
                        let _ = resp.send(total_tokens_impl(&conn, start_ts, end_ts));
                    }
                    AnalyticsRequest::Requests {
                        start_ts,
                        end_ts,
                        resp,
                    } => {
                        let _ = resp.send(requests_impl(&conn, start_ts, end_ts));
                    }
                    AnalyticsRequest::Tokens {
                        start_ts,
                        end_ts,
                        resp,
                    } => {
                        let _ = resp.send(tokens_impl(&conn, start_ts, end_ts));
                    }
                    AnalyticsRequest::ModelDist {
                        start_ts,
                        end_ts,
                        resp,
                    } => {
                        let _ = resp.send(model_dist_impl(&conn, start_ts, end_ts));
                    }
                    AnalyticsRequest::TokenDist {
                        start_ts,
                        end_ts,
                        resp,
                    } => {
                        let _ = resp.send(token_dist_impl(&conn, start_ts, end_ts));
                    }
                    AnalyticsRequest::QueryProxyLogs { params, resp } => {
                        let _ = resp.send(query_proxy_logs_impl(&conn, *params));
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

    analytics_method!(total_requests, TotalRequests, u64);
    analytics_method!(total_tokens, TotalTokens, u64);
    analytics_method!(requests, Requests, Vec<BucketEntry>);
    analytics_method!(tokens, Tokens, Vec<BucketEntry>);
    analytics_method!(model_dist, ModelDist, Vec<ModelDistEntry>);
    analytics_method!(token_dist, TokenDist, Vec<TokenDistEntry>);

    pub async fn query_proxy_logs(&self, params: ProxyLogQueryParams) -> ProxyLogQueryResult {
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
            return ProxyLogQueryResult {
                items: Vec::new(),
                total: 0,
            };
        }
        match timeout(self.timeout, rx).await {
            Ok(inner) => inner.unwrap_or_else(|_| {
                warn!("[analytics] query_proxy_logs oneshot cancelled");
                ProxyLogQueryResult {
                    items: Vec::new(),
                    total: 0,
                }
            }),
            Err(_) => {
                warn!("[analytics] query_proxy_logs timed out");
                ProxyLogQueryResult {
                    items: Vec::new(),
                    total: 0,
                }
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
        .expect("start_ts validated by caller")
        .naive_utc();
    let end = DateTime::from_timestamp(end_ts, 0)
        .expect("end_ts validated by caller")
        .naive_utc();
    (start, end)
}

fn total_requests_impl(conn: &Connection, start_ts: i64, end_ts: i64) -> u64 {
    let (start, end) = ts_range(start_ts, end_ts);
    conn.query_row(
        "SELECT COUNT(*) FROM proxy_log WHERE timestamp >= ?1 AND timestamp < ?2",
        params![start, end],
        |row| row.get::<_, u64>(0),
    )
    .unwrap_or_else(|e| {
        warn!("[analytics] total_requests failed: {e}");
        0
    })
}

fn total_tokens_impl(conn: &Connection, start_ts: i64, end_ts: i64) -> u64 {
    let (start, end) = ts_range(start_ts, end_ts);
    conn.query_row(
        "SELECT COALESCE(SUM(total_tokens), 0) FROM proxy_log WHERE timestamp >= ?1 AND timestamp < ?2",
        params![start, end],
        |row| row.get::<_, u64>(0),
    )
    .unwrap_or_else(|e| {
        warn!("[analytics] total_tokens failed: {e}");
        0
    })
}

fn requests_impl(conn: &Connection, start_ts: i64, end_ts: i64) -> Vec<BucketEntry> {
    let (start, end) = ts_range(start_ts, end_ts);
    let mut stmt = match conn.prepare(
        "SELECT epoch(DATE_TRUNC('hour', timestamp)) AS bucket_ts, COUNT(*) AS count \
         FROM proxy_log WHERE timestamp >= ?1 AND timestamp < ?2 \
         GROUP BY bucket_ts ORDER BY bucket_ts",
    ) {
        Ok(s) => s,
        Err(e) => {
            warn!("[analytics] requests prepare failed: {e}");
            return Vec::new();
        }
    };
    let rows = match stmt.query_map(params![start, end], |row| {
        Ok(BucketEntry {
            timestamp: row.get::<_, f64>(0)? as i64,
            count: row.get::<_, i64>(1)? as u64,
        })
    }) {
        Ok(r) => r,
        Err(e) => {
            warn!("[analytics] requests query failed: {e}");
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for r in rows.flatten() {
        out.push(r);
    }
    out
}

fn tokens_impl(conn: &Connection, start_ts: i64, end_ts: i64) -> Vec<BucketEntry> {
    let (start, end) = ts_range(start_ts, end_ts);
    let mut stmt = match conn.prepare(
        "SELECT epoch(DATE_TRUNC('hour', timestamp)) AS bucket_ts, \
                COALESCE(SUM(total_tokens), 0) AS count \
         FROM proxy_log WHERE timestamp >= ?1 AND timestamp < ?2 \
         GROUP BY bucket_ts ORDER BY bucket_ts",
    ) {
        Ok(s) => s,
        Err(e) => {
            warn!("[analytics] tokens prepare failed: {e}");
            return Vec::new();
        }
    };
    let rows = match stmt.query_map(params![start, end], |row| {
        Ok(BucketEntry {
            timestamp: row.get::<_, f64>(0)? as i64,
            count: row.get::<_, i64>(1)? as u64,
        })
    }) {
        Ok(r) => r,
        Err(e) => {
            warn!("[analytics] tokens query failed: {e}");
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for r in rows.flatten() {
        out.push(r);
    }
    out
}

fn model_dist_impl(conn: &Connection, start_ts: i64, end_ts: i64) -> Vec<ModelDistEntry> {
    let (start, end) = ts_range(start_ts, end_ts);
    let mut stmt = match conn.prepare(
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

fn token_dist_impl(conn: &Connection, start_ts: i64, end_ts: i64) -> Vec<TokenDistEntry> {
    let (start, end) = ts_range(start_ts, end_ts);
    let (prompt, completion, cached): (i64, i64, i64) = match conn.query_row(
        "SELECT COALESCE(SUM(prompt_tokens), 0), \
                COALESCE(SUM(completion_tokens), 0), \
                COALESCE(SUM(cached_tokens), 0) \
         FROM proxy_log WHERE timestamp >= ?1 AND timestamp < ?2",
        params![start, end],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ) {
        Ok(v) => v,
        Err(e) => {
            warn!("[analytics] token_dist query failed: {e}");
            return Vec::new();
        }
    };

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

fn query_proxy_logs_impl(conn: &Connection, params: ProxyLogQueryParams) -> ProxyLogQueryResult {
    use duckdb::types::ToSql;

    let mut conditions: Vec<String> = Vec::new();
    let mut values: Vec<Box<dyn ToSql>> = Vec::new();

    let push_naive = |values: &mut Vec<Box<dyn ToSql>>, ts: i64| {
        let naive = DateTime::from_timestamp(ts, 0)
            .expect("ts validated by caller")
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
    if let Some(ref username) = params.username
        && !username.is_empty()
    {
        conditions.push("username = ?".into());
        values.push(Box::new(username.clone()));
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

    // Data query
    let offset = (params.page.saturating_sub(1)) * params.per_page;
    let data_sql = format!(
        "SELECT timestamp, username, api_key_name, model_name, provider_name, \
         prompt_tokens, completion_tokens, total_tokens, cached_tokens, latency_ms, status, \
         endpoint, is_streaming, time_to_first_token_ms, upstream_model, provider_type, \
         response_body_size, error_message, client_ip \
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
                timestamp: row.get::<_, i64>(0)?,
                username: row.get(1)?,
                api_key_name: row.get(2)?,
                model_name: row.get(3)?,
                provider_name: row.get(4)?,
                prompt_tokens: row.get(5)?,
                completion_tokens: row.get(6)?,
                total_tokens: row.get(7)?,
                cached_tokens: row.get(8)?,
                latency_ms: row.get(9)?,
                status: row.get(10)?,
                endpoint: row.get(11)?,
                is_streaming: row.get(12)?,
                time_to_first_token_ms: row.get(13)?,
                upstream_model: row.get(14)?,
                provider_type: row.get(15)?,
                response_body_size: row.get(16)?,
                error_message: row.get(17)?,
                client_ip: row.get(18)?,
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
