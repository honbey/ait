use duckdb::{Connection, params};
use std::sync::mpsc;
use tokio::sync::oneshot;

#[derive(Clone)]
pub struct Analytics {
    tx: mpsc::Sender<AnalyticsRequest>,
}

enum AnalyticsRequest {
    QueryStats {
        resp: oneshot::Sender<(u64, u64)>,
    },
    TotalRequests {
        days: i64,
        resp: oneshot::Sender<u64>,
    },
    TotalTokens {
        resp: oneshot::Sender<u64>,
    },
    Shutdown,
}

impl Analytics {
    pub fn new(conn: Connection) -> Self {
        let (tx, rx) = mpsc::channel::<AnalyticsRequest>();
        std::thread::spawn(move || {
            for req in rx {
                match req {
                    AnalyticsRequest::QueryStats { resp } => {
                        let _ = resp.send(query_stats_impl(&conn));
                    }
                    AnalyticsRequest::TotalRequests { days, resp } => {
                        let _ = resp.send(total_requests_impl(&conn, days));
                    }
                    AnalyticsRequest::TotalTokens { resp } => {
                        let _ = resp.send(total_tokens_impl(&conn));
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

    pub async fn query_stats(&self) -> (u64, u64) {
        let (resp, rx) = oneshot::channel();
        if self.tx.send(AnalyticsRequest::QueryStats { resp }).is_err() {
            return (0, 0);
        }
        rx.await.unwrap_or((0, 0))
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

    pub async fn total_tokens(&self) -> u64 {
        let (resp, rx) = oneshot::channel();
        if self
            .tx
            .send(AnalyticsRequest::TotalTokens { resp })
            .is_err()
        {
            return 0;
        }
        rx.await.unwrap_or(0)
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(AnalyticsRequest::Shutdown);
    }
}

fn query_stats_impl(conn: &Connection) -> (u64, u64) {
    let count = conn
        .query_row("SELECT COUNT(*) FROM proxy_log", [], |row| {
            row.get::<_, u64>(0)
        })
        .unwrap_or(0);
    let tokens = conn
        .query_row(
            "SELECT COALESCE(SUM(total_tokens), 0) FROM proxy_log",
            [],
            |row| row.get::<_, u64>(0),
        )
        .unwrap_or(0);
    (count, tokens)
}

fn total_requests_impl(conn: &Connection, days: i64) -> u64 {
    if days > 0 {
        conn.query_row(
            "SELECT COUNT(*) FROM proxy_log WHERE timestamp >= CURRENT_DATE - ?1",
            params![days],
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

fn total_tokens_impl(conn: &Connection) -> u64 {
    conn.query_row(
        "SELECT COALESCE(SUM(total_tokens), 0) FROM proxy_log",
        [],
        |row| row.get::<_, u64>(0),
    )
    .unwrap_or(0)
}
