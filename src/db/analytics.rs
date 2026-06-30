use chrono::{DateTime, Utc};
use duckdb::{Connection, params};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use tokio::sync::oneshot;
use tracing::{error, warn};

use super::models::BucketEntry;

#[derive(Clone)]
pub struct Analytics {
    tx: mpsc::Sender<AnalyticsRequest>,
    handle: Arc<Mutex<Option<JoinHandle<()>>>>,
}

enum AnalyticsRequest {
    TotalRequests {
        days: i64,
        resp: oneshot::Sender<u64>,
    },
    TotalTokens {
        days: i64,
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
    Shutdown,
}

impl Analytics {
    pub fn new(conn: Connection) -> Self {
        let (tx, rx) = mpsc::channel::<AnalyticsRequest>();
        let handle = std::thread::spawn(move || {
            for req in rx {
                match req {
                    AnalyticsRequest::TotalRequests { days, resp } => {
                        let _ = resp.send(total_requests_impl(&conn, days));
                    }
                    AnalyticsRequest::TotalTokens { days, resp } => {
                        let _ = resp.send(total_tokens_impl(&conn, days));
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
        }
    }

    pub async fn total_requests(&self, days: i64) -> u64 {
        let (resp, rx) = oneshot::channel();
        if self
            .tx
            .send(AnalyticsRequest::TotalRequests { days, resp })
            .is_err()
        {
            error!("[analytics] total_requests channel send failed");
            return 0;
        }
        rx.await.unwrap_or_else(|_| {
            warn!("[analytics] total_requests oneshot cancelled");
            0
        })
    }

    pub async fn total_tokens(&self, days: i64) -> u64 {
        let (resp, rx) = oneshot::channel();
        if self
            .tx
            .send(AnalyticsRequest::TotalTokens { days, resp })
            .is_err()
        {
            error!("[analytics] total_tokens channel send failed");
            return 0;
        }
        rx.await.unwrap_or_else(|_| {
            warn!("[analytics] total_tokens oneshot cancelled");
            0
        })
    }

    pub async fn requests(&self, start_ts: i64, end_ts: i64) -> Vec<BucketEntry> {
        let (resp, rx) = oneshot::channel();
        if self
            .tx
            .send(AnalyticsRequest::Requests {
                start_ts,
                end_ts,
                resp,
            })
            .is_err()
        {
            error!("[analytics] requests channel send failed");
            return Vec::new();
        }
        rx.await.unwrap_or_else(|_| {
            warn!("[analytics] requests oneshot cancelled");
            Vec::new()
        })
    }

    pub async fn tokens(&self, start_ts: i64, end_ts: i64) -> Vec<BucketEntry> {
        let (resp, rx) = oneshot::channel();
        if self
            .tx
            .send(AnalyticsRequest::Tokens {
                start_ts,
                end_ts,
                resp,
            })
            .is_err()
        {
            error!("[analytics] tokens channel send failed");
            return Vec::new();
        }
        rx.await.unwrap_or_else(|_| {
            warn!("[analytics] tokens oneshot cancelled");
            Vec::new()
        })
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

fn total_requests_impl(conn: &Connection, days: i64) -> u64 {
    let cutoff = (Utc::now() - chrono::Duration::days(days)).naive_utc();
    conn.query_row(
        "SELECT COUNT(*) FROM proxy_log WHERE timestamp >= ?1",
        params![cutoff],
        |row| row.get::<_, u64>(0),
    )
    .unwrap_or_else(|e| {
        warn!("[analytics] total_requests failed: {e}");
        0
    })
}

fn total_tokens_impl(conn: &Connection, days: i64) -> u64 {
    let cutoff = (Utc::now() - chrono::Duration::days(days)).naive_utc();
    conn.query_row(
        "SELECT COALESCE(SUM(total_tokens), 0) FROM proxy_log WHERE timestamp >= ?1",
        params![cutoff],
        |row| row.get::<_, u64>(0),
    )
    .unwrap_or_else(|e| {
        warn!("[analytics] total_tokens failed: {e}");
        0
    })
}

fn requests_impl(conn: &Connection, start_ts: i64, end_ts: i64) -> Vec<BucketEntry> {
    let start = match DateTime::from_timestamp(start_ts, 0) {
        Some(t) => t.naive_utc(),
        None => {
            warn!("[analytics] requests invalid start_ts: {start_ts}");
            return Vec::new();
        }
    };
    let end = match DateTime::from_timestamp(end_ts, 0) {
        Some(t) => t.naive_utc(),
        None => {
            warn!("[analytics] requests invalid end_ts: {end_ts}");
            return Vec::new();
        }
    };
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
            timestamp: row.get::<_, f64>(0)?,
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
    let start = match DateTime::from_timestamp(start_ts, 0) {
        Some(t) => t.naive_utc(),
        None => {
            warn!("[analytics] tokens invalid start_ts: {start_ts}");
            return Vec::new();
        }
    };
    let end = match DateTime::from_timestamp(end_ts, 0) {
        Some(t) => t.naive_utc(),
        None => {
            warn!("[analytics] tokens invalid end_ts: {end_ts}");
            return Vec::new();
        }
    };
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
            timestamp: row.get::<_, f64>(0)?,
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
