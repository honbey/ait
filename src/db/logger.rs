use chrono::Utc;
use duckdb::{Connection, Result, params};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tracing::{error, info, warn};

use super::analytics::Analytics;
use super::models::{AccessEvent, AuditEvent, DailyRequests, DailyTokens, LogEvent, ProxyEvent};

const FLUSH_INTERVAL: Duration = Duration::from_secs(10);
const FLUSH_BATCH: usize = 100;
const CHANNEL_CAP: usize = 10_000;
const RETENTION_DAYS: i64 = 30;
const RETENTION_EVERY: usize = 100;

#[derive(Clone)]
pub struct LogManager {
    sender: mpsc::SyncSender<LogEvent>,
    analytics: Analytics,
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
        let analytics = Analytics::new(conn.try_clone()?);

        thread::spawn(move || match conn.try_clone() {
            Ok(worker_conn) => {
                if let Err(e) = worker_loop(receiver, worker_conn) {
                    error!("[logs] worker exited with error: {e}");
                }
            }
            Err(e) => error!("[logs] failed to clone worker connection: {e}"),
        });

        Ok(Self { sender, analytics })
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

    pub async fn total_requests(&self, days: i64) -> u64 {
        self.analytics.total_requests(days).await
    }

    pub async fn total_tokens(&self, days: i64) -> u64 {
        self.analytics.total_tokens(days).await
    }

    pub async fn daily_requests(&self, days: i64) -> Vec<DailyRequests> {
        self.analytics.daily_requests(days).await
    }

    pub async fn daily_tokens(&self, days: i64) -> Vec<DailyTokens> {
        self.analytics.daily_tokens(days).await
    }

    pub fn shutdown(&self) {
        while self.sender.try_send(LogEvent::Shutdown).is_err() {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        self.analytics.shutdown();
    }
}

fn worker_loop(receiver: mpsc::Receiver<LogEvent>, conn: Connection) -> Result<()> {
    let mut buffer: Vec<LogEvent> = Vec::with_capacity(FLUSH_BATCH);
    let mut flush_count = 0u64;

    loop {
        let mut shutdown = false;

        match receiver.recv_timeout(FLUSH_INTERVAL) {
            Ok(LogEvent::Shutdown) => shutdown = true,
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
                let _ = conn.execute_batch("CHECKPOINT");
                return Ok(());
            }
        }

        while let Ok(event) = receiver.try_recv() {
            match event {
                LogEvent::Shutdown => shutdown = true,
                other => buffer.push(other),
            }
        }

        if buffer.len() >= FLUSH_BATCH {
            flush_buffer(&conn, &mut buffer, &mut flush_count);
        }

        if shutdown {
            if !buffer.is_empty() {
                flush_events(&conn, &buffer);
            }
            let _ = conn.execute_batch("CHECKPOINT");
            return Ok(());
        }
    }
}

fn flush_buffer(conn: &Connection, buffer: &mut Vec<LogEvent>, flush_count: &mut u64) {
    if flush_events(conn, buffer) {
        *flush_count += 1;
        if flush_count.is_multiple_of(RETENTION_EVERY as u64) {
            let _ = conn.execute_batch("CHECKPOINT");
            cleanup_expired(conn);
        }
        buffer.clear();
    }
}

fn flush_events(conn: &Connection, events: &[LogEvent]) -> bool {
    let mut access = Vec::new();
    let mut proxy = Vec::new();
    let mut audit = Vec::new();
    for event in events {
        match event {
            LogEvent::Access(e) => access.push(e),
            LogEvent::Proxy(e) => proxy.push(e),
            LogEvent::Audit(e) => audit.push(e),
            LogEvent::Shutdown => {}
        }
    }

    if let Err(e) = conn.execute_batch("BEGIN TRANSACTION") {
        error!("[logs] begin tx failed: {e}");
        return false;
    }

    if flush_accesses(conn, &access)
        .map_err(|e| error!("[logs] flush access_log failed: {e}"))
        .is_ok()
        && flush_proxies(conn, &proxy)
            .map_err(|e| error!("[logs] flush proxy_log failed: {e}"))
            .is_ok()
        && flush_audits(conn, &audit)
            .map_err(|e| error!("[logs] flush audit_log failed: {e}"))
            .is_ok()
    {
        if let Err(e) = conn.execute_batch("COMMIT") {
            error!("[logs] commit failed: {e}");
            return false;
        }
        true
    } else {
        if let Err(e) = conn.execute_batch("ROLLBACK") {
            error!("[logs] rollback failed: {e}");
        }
        false
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
         prompt_tokens, completion_tokens, total_tokens, cached_tokens, latency_ms, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
            e.cached_tokens,
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
