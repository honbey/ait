use chrono::Utc;
use duckdb::{Connection, Result, params};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tracing::{error, info, warn};

use super::models::{AccessEvent, AuditEvent, LogEvent, ProxyEvent};

const FLUSH_INTERVAL: Duration = Duration::from_secs(10);
const FLUSH_BATCH: usize = 100;
const CHANNEL_CAP: usize = 10_000;
const RETENTION_DAYS: i64 = 30;
const RETENTION_EVERY: usize = 100;

#[derive(Clone)]
pub struct LogManager {
    sender: mpsc::SyncSender<LogEvent>,
    db_path: String,
}

impl LogManager {
    pub fn new(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS access_log (
                timestamp  TIMESTAMP NOT NULL,
                method     VARCHAR NOT NULL,
                path       VARCHAR NOT NULL,
                status     INT NOT NULL,
                latency_ms BIGINT NOT NULL,
                client_ip  VARCHAR,
                username   VARCHAR
            );
            CREATE TABLE IF NOT EXISTS proxy_log (
                timestamp         TIMESTAMP NOT NULL,
                username          VARCHAR,
                model_name        VARCHAR NOT NULL,
                provider_name     VARCHAR NOT NULL,
                prompt_tokens     BIGINT,
                completion_tokens BIGINT,
                total_tokens      BIGINT,
                cached_tokens     BIGINT,
                latency_ms        BIGINT NOT NULL,
                status            VARCHAR NOT NULL
            );
            CREATE TABLE IF NOT EXISTS audit_log (
                timestamp   TIMESTAMP NOT NULL,
                username    VARCHAR NOT NULL,
                action      VARCHAR NOT NULL,
                resource    VARCHAR NOT NULL,
                resource_id VARCHAR NOT NULL,
                detail      VARCHAR
            );",
        )?;

        let (sender, receiver) = mpsc::sync_channel(CHANNEL_CAP);
        let db_path_owned = db_path.to_string();

        thread::spawn(move || {
            if let Err(e) = worker_loop(&db_path_owned, receiver) {
                error!("[logs] worker exited with error: {e}");
            }
        });

        Ok(Self {
            sender,
            db_path: db_path.to_string(),
        })
    }

    pub fn log_access(&self, event: AccessEvent) {
        if let Err(e) = self.sender.try_send(LogEvent::Access(event)) {
            warn!("[logs] access buffer full, dropping event: {e}");
        }
    }

    pub fn log_proxy(&self, event: ProxyEvent) {
        if let Err(e) = self.sender.try_send(LogEvent::Proxy(event)) {
            warn!("[logs] proxy buffer full, dropping event: {e}");
        }
    }

    pub fn log_audit(&self, event: AuditEvent) {
        if let Err(e) = self.sender.try_send(LogEvent::Audit(event)) {
            warn!("[logs] audit buffer full, dropping event: {e}");
        }
    }

    pub fn query_stats(&self) -> (u64, u64) {
        let conn = match Connection::open(&self.db_path) {
            Ok(c) => c,
            Err(e) => {
                error!("[logs] failed to open DuckDB for stats: {e}");
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
}

fn worker_loop(db_path: &str, receiver: mpsc::Receiver<LogEvent>) -> Result<()> {
    let conn = Connection::open(db_path)?;
    let mut buffer: Vec<LogEvent> = Vec::with_capacity(FLUSH_BATCH);
    let mut flush_count = 0u64;

    loop {
        match receiver.recv_timeout(FLUSH_INTERVAL) {
            Ok(event) => buffer.push(event),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !buffer.is_empty() {
                    flush_buffer(&conn, &mut buffer, &mut flush_count);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if !buffer.is_empty() {
                    flush_events(&conn, &buffer);
                }
                return Ok(());
            }
        }

        while let Ok(event) = receiver.try_recv() {
            buffer.push(event);
        }

        if buffer.len() >= FLUSH_BATCH {
            flush_buffer(&conn, &mut buffer, &mut flush_count);
        }
    }
}

fn flush_buffer(conn: &Connection, buffer: &mut Vec<LogEvent>, flush_count: &mut u64) {
    flush_events(conn, buffer);
    *flush_count += 1;
    if flush_count.is_multiple_of(RETENTION_EVERY as u64) {
        cleanup_expired(conn);
    }
    buffer.clear();
}

fn flush_events(conn: &Connection, events: &[LogEvent]) {
    let mut access = Vec::new();
    let mut proxy = Vec::new();
    let mut audit = Vec::new();
    for event in events {
        match event {
            LogEvent::Access(e) => access.push(e),
            LogEvent::Proxy(e) => proxy.push(e),
            LogEvent::Audit(e) => audit.push(e),
        }
    }

    if let Err(e) = conn.execute_batch("BEGIN TRANSACTION") {
        error!("[logs] begin tx failed: {e}");
        return;
    }

    let ok = flush_accesses(conn, &access).is_ok()
        && flush_proxies(conn, &proxy).is_ok()
        && flush_audits(conn, &audit).is_ok();

    if ok {
        if let Err(e) = conn.execute_batch("COMMIT") {
            error!("[logs] commit failed: {e}");
        }
    } else {
        if let Err(e) = conn.execute_batch("ROLLBACK") {
            error!("[logs] rollback failed: {e}");
        }
    }
}

fn flush_accesses(conn: &Connection, events: &[&AccessEvent]) -> Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    let mut stmt = conn.prepare_cached(
        "INSERT INTO access_log (timestamp, method, path, status, latency_ms, client_ip, username)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    for e in events {
        stmt.execute(params![
            e.timestamp.naive_utc(),
            e.method,
            e.path,
            e.status,
            e.latency_ms,
            e.client_ip,
            e.username,
        ])?;
    }
    Ok(())
}

fn flush_proxies(conn: &Connection, events: &[&ProxyEvent]) -> Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    let mut stmt = conn.prepare_cached(
        "INSERT INTO proxy_log (timestamp, username, model_name, provider_name,
         prompt_tokens, completion_tokens, total_tokens, latency_ms, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    for e in events {
        stmt.execute(params![
            e.timestamp.naive_utc(),
            e.username,
            e.model_name,
            e.provider_name,
            e.prompt_tokens,
            e.completion_tokens,
            e.total_tokens,
            e.latency_ms,
            e.status,
        ])?;
    }
    Ok(())
}

fn flush_audits(conn: &Connection, events: &[&AuditEvent]) -> Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    let mut stmt = conn.prepare_cached(
        "INSERT INTO audit_log (timestamp, username, action, resource, resource_id, detail)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for e in events {
        stmt.execute(params![
            e.timestamp.naive_utc(),
            e.username,
            e.action,
            e.resource,
            e.resource_id,
            e.detail,
        ])?;
    }
    Ok(())
}

fn cleanup_expired(conn: &Connection) {
    let cutoff = (Utc::now() - chrono::Duration::days(RETENTION_DAYS)).naive_utc();
    for table in &["access_log", "proxy_log", "audit_log"] {
        match conn.execute(
            &format!("DELETE FROM {table} WHERE timestamp < ?1"),
            params![cutoff],
        ) {
            Ok(n) if n > 0 => info!("[logs] cleanup {table}: deleted {n} rows"),
            Ok(_) => {}
            Err(e) => warn!("[logs] cleanup {table} failed: {e}"),
        }
    }
}
