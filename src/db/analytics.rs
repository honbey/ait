use duckdb::{Connection, params};
use tracing::error;

#[derive(Clone)]
pub struct Analytics {
    db_path: String,
}

impl Analytics {
    pub fn new(db_path: &str) -> Self {
        Self {
            db_path: db_path.to_string(),
        }
    }

    pub fn total_requests(&self, days: i64) -> u64 {
        let conn = match Connection::open(&self.db_path) {
            Ok(c) => c,
            Err(e) => {
                error!("[analytics] failed to open DuckDB: {e}");
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
        let conn = match Connection::open(&self.db_path) {
            Ok(c) => c,
            Err(e) => {
                error!("[analytics] failed to open DuckDB: {e}");
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
