use chrono::{Duration, Utc};
use duckdb::{Connection, params};
use std::sync::mpsc;
use tokio::sync::oneshot;

use super::models::{DailyRequests, DailyTokens};

#[derive(Clone)]
pub struct Analytics {
    tx: mpsc::Sender<AnalyticsRequest>,
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
    DailyRequests {
        days: i64,
        resp: oneshot::Sender<Vec<DailyRequests>>,
    },
    DailyTokens {
        days: i64,
        resp: oneshot::Sender<Vec<DailyTokens>>,
    },
    Shutdown,
}

impl Analytics {
    pub fn new(conn: Connection) -> Self {
        let (tx, rx) = mpsc::channel::<AnalyticsRequest>();
        std::thread::spawn(move || {
            for req in rx {
                match req {
                    AnalyticsRequest::TotalRequests { days, resp } => {
                        let _ = resp.send(total_requests_impl(&conn, days));
                    }
                    AnalyticsRequest::TotalTokens { days, resp } => {
                        let _ = resp.send(total_tokens_impl(&conn, days));
                    }
                    AnalyticsRequest::DailyRequests { days, resp } => {
                        let _ = resp.send(daily_requests_impl(&conn, days));
                    }
                    AnalyticsRequest::DailyTokens { days, resp } => {
                        let _ = resp.send(daily_tokens_impl(&conn, days));
                    }
                    AnalyticsRequest::Shutdown => {
                        let _ = conn.execute_batch("CHECKPOINT");
                        break;
                    }
                }
            }
        });
        Self { tx }
    }

    pub async fn total_requests(&self, days: i64) -> u64 {
        let (resp, rx) = oneshot::channel();
        if self
            .tx
            .send(AnalyticsRequest::TotalRequests { days, resp })
            .is_err()
        {
            return 0;
        }
        rx.await.unwrap_or(0)
    }

    pub async fn total_tokens(&self, days: i64) -> u64 {
        let (resp, rx) = oneshot::channel();
        if self
            .tx
            .send(AnalyticsRequest::TotalTokens { days, resp })
            .is_err()
        {
            return 0;
        }
        rx.await.unwrap_or(0)
    }

    pub async fn daily_requests(&self, days: i64) -> Vec<DailyRequests> {
        let (resp, rx) = oneshot::channel();
        if self
            .tx
            .send(AnalyticsRequest::DailyRequests { days, resp })
            .is_err()
        {
            return Vec::new();
        }
        rx.await.unwrap_or_default()
    }

    pub async fn daily_tokens(&self, days: i64) -> Vec<DailyTokens> {
        let (resp, rx) = oneshot::channel();
        if self
            .tx
            .send(AnalyticsRequest::DailyTokens { days, resp })
            .is_err()
        {
            return Vec::new();
        }
        rx.await.unwrap_or_default()
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(AnalyticsRequest::Shutdown);
    }
}

fn total_requests_impl(conn: &Connection, days: i64) -> u64 {
    if days > 0 {
        let cutoff = (Utc::now() - Duration::days(days)).naive_utc();
        conn.query_row(
            "SELECT COUNT(*) FROM proxy_log WHERE timestamp >= ?1",
            params![cutoff],
            |row| row.get::<_, u64>(0),
        )
        .unwrap_or(0)
    } else {
        conn.query_row("SELECT COUNT(*) FROM proxy_log", [], |row| {
            row.get::<_, u64>(0)
        })
        .unwrap_or(0)
    }
}

fn total_tokens_impl(conn: &Connection, days: i64) -> u64 {
    if days > 0 {
        let cutoff = (Utc::now() - Duration::days(days)).naive_utc();
        conn.query_row(
            "SELECT COALESCE(SUM(total_tokens), 0) FROM proxy_log WHERE timestamp >= ?1",
            params![cutoff],
            |row| row.get::<_, u64>(0),
        )
        .unwrap_or(0)
    } else {
        conn.query_row(
            "SELECT COALESCE(SUM(total_tokens), 0) FROM proxy_log",
            [],
            |row| row.get::<_, u64>(0),
        )
        .unwrap_or(0)
    }
}

fn daily_requests_impl(conn: &Connection, days: i64) -> Vec<DailyRequests> {
    if days > 0 {
        let cutoff = (Utc::now() - Duration::days(days)).naive_utc();
        if let Ok(mut stmt) = conn.prepare(
            "SELECT DATE(timestamp) as date, COUNT(*) as count FROM proxy_log WHERE timestamp >= ?1 GROUP BY date ORDER BY date",
        ) {
            if let Ok(rows) = stmt.query_map(params![cutoff], |row| {
                Ok(DailyRequests {
                    date: row.get::<_, chrono::NaiveDate>(0)?.to_string(),
                    count: row.get::<_, i64>(1)? as u64,
                })
            }) {
                let mut out = Vec::new();
                for r in rows.flatten() {
                    out.push(r);
                }
                out
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    } else {
        if let Ok(mut stmt) = conn.prepare(
            "SELECT DATE(timestamp) as date, COUNT(*) as count FROM proxy_log GROUP BY date ORDER BY date",
        ) {
            if let Ok(rows) = stmt.query_map([], |row| {
                Ok(DailyRequests {
                    date: row.get::<_, chrono::NaiveDate>(0)?.to_string(),
                    count: row.get::<_, i64>(1)? as u64,
                })
            }) {
                let mut out = Vec::new();
                for r in rows.flatten() {
                    out.push(r);
                }
                out
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    }
}

fn daily_tokens_impl(conn: &Connection, days: i64) -> Vec<DailyTokens> {
    if days > 0 {
        let cutoff = (Utc::now() - Duration::days(days)).naive_utc();
        if let Ok(mut stmt) = conn.prepare(
            "SELECT DATE(timestamp) as date, COALESCE(SUM(total_tokens), 0) as tokens FROM proxy_log WHERE timestamp >= ?1 GROUP BY date ORDER BY date",
        ) {
            if let Ok(rows) = stmt.query_map(params![cutoff], |row| {
                Ok(DailyTokens {
                    date: row.get::<_, chrono::NaiveDate>(0)?.to_string(),
                    tokens: row.get::<_, i64>(1)? as u64,
                })
            }) {
                let mut out = Vec::new();
                for r in rows.flatten() {
                    out.push(r);
                }
                out
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    } else {
        if let Ok(mut stmt) = conn.prepare(
            "SELECT DATE(timestamp) as date, COALESCE(SUM(total_tokens), 0) as tokens FROM proxy_log GROUP BY date ORDER BY date",
        ) {
            if let Ok(rows) = stmt.query_map([], |row| {
                Ok(DailyTokens {
                    date: row.get::<_, chrono::NaiveDate>(0)?.to_string(),
                    tokens: row.get::<_, i64>(1)? as u64,
                })
            }) {
                let mut out = Vec::new();
                for r in rows.flatten() {
                    out.push(r);
                }
                out
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    }
}
