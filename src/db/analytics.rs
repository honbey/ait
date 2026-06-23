use duckdb::{Connection, params};
use std::sync::{Arc, Mutex};
use tracing::error;

#[derive(Clone)]
pub struct Analytics {
    conn: Arc<Mutex<Connection>>,
}

impl Analytics {
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    pub fn query_stats(&self) -> (u64, u64) {
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(e) => {
                error!("[analytics] lock failed: {e}");
                return (0, 0);
            }
        };
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

    pub fn total_requests(&self, days: i64) -> u64 {
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(e) => {
                error!("[analytics] lock failed: {e}");
                return 0;
            }
        };
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

    pub fn total_tokens(&self) -> u64 {
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(e) => {
                error!("[analytics] lock failed: {e}");
                return 0;
            }
        };
        conn.query_row(
            "SELECT COALESCE(SUM(total_tokens), 0) FROM proxy_log",
            [],
            |row| row.get::<_, u64>(0),
        )
        .unwrap_or(0)
    }
}
