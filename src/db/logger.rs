use chrono::Utc;
use duckdb::{Connection, Result, params};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tracing::{error, info, warn};

use super::analytics::Analytics;
use super::loki::LokiSink;
use super::models::{
    AccessEvent, AuditEvent, BucketEntry, LogEvent, ModelDistEntry, ProxyEvent,
    ProxyLogQueryParams, ProxyLogQueryResult, TokenDistEntry,
};

#[derive(Clone)]
pub struct LogManager {
    sender: mpsc::SyncSender<LogEvent>,
    analytics: Analytics,
    worker_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    loki: Option<LokiSink>,
}

/// Create the log tables and indexes if they do not exist yet.
pub(crate) fn create_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS access_log (
            timestamp  TIMESTAMP NOT NULL,
            request_id VARCHAR NOT NULL,
            method     VARCHAR NOT NULL,
            path       VARCHAR NOT NULL,
            status     INT NOT NULL,
            latency_ms BIGINT NOT NULL,
            client_ip  VARCHAR
        );
        CREATE TABLE IF NOT EXISTS proxy_log (
            timestamp         TIMESTAMP NOT NULL,
            request_id        VARCHAR NOT NULL,
            api_key_name      VARCHAR,
            model_name        VARCHAR NOT NULL,
            provider_name     VARCHAR NOT NULL,
            prompt_tokens     BIGINT,
            completion_tokens BIGINT,
            total_tokens      BIGINT,
            cached_tokens     BIGINT,
            latency_ms        BIGINT NOT NULL,
            status                 VARCHAR NOT NULL,
            endpoint               VARCHAR NOT NULL DEFAULT '',
            is_streaming           BOOLEAN NOT NULL DEFAULT false,
            time_to_first_token_ms BIGINT,
            upstream_model         VARCHAR NOT NULL DEFAULT '',
            provider_type          VARCHAR NOT NULL DEFAULT '',
            response_body_size     BIGINT,
            error_message          VARCHAR,
            client_ip              VARCHAR
        );
        CREATE TABLE IF NOT EXISTS audit_log (
            timestamp   TIMESTAMP NOT NULL,
            request_id  VARCHAR NOT NULL,
            action      VARCHAR NOT NULL,
            resource    VARCHAR NOT NULL,
            resource_id VARCHAR NOT NULL,
            detail      VARCHAR
        );
        CREATE INDEX IF NOT EXISTS idx_access_log_timestamp ON access_log(timestamp);
        CREATE INDEX IF NOT EXISTS idx_proxy_log_timestamp ON proxy_log(timestamp);
        CREATE INDEX IF NOT EXISTS idx_audit_log_timestamp ON audit_log(timestamp);",
    )
}

impl LogManager {
    pub fn new(config: &crate::config::LogConfig) -> Result<Self> {
        let conn = Connection::open(&config.path)?;
        create_schema(&conn)?;

        let flush_interval = Duration::from_secs(config.flush_interval_secs);
        let flush_batch = config.flush_batch;
        let retention_every = config.retention_every;
        let retention_days = config.retention_days;
        let (sender, receiver) = mpsc::sync_channel(config.channel_cap as usize);
        let analytics = Analytics::new(conn.try_clone()?, config.analytics_timeout_secs);

        let worker_conn = conn.try_clone()?;
        let handle = thread::spawn(move || {
            if let Err(e) = worker_loop(
                receiver,
                worker_conn,
                flush_batch,
                flush_interval,
                retention_every,
                retention_days,
            ) {
                error!("[logs] worker exited with error: {e}");
            }
        });

        let loki = if config.loki.enabled && !config.loki.url.is_empty() {
            match LokiSink::new(&config.loki) {
                Ok(sink) => {
                    info!("[loki] push enabled -> {}", config.loki.url);
                    Some(sink)
                }
                Err(e) => {
                    warn!("[loki] init failed, sink disabled: {e}");
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            sender,
            analytics,
            worker_handle: Arc::new(Mutex::new(Some(handle))),
            loki,
        })
    }

    pub fn log_access(&self, event: AccessEvent) {
        if let Some(loki) = &self.loki {
            loki.send(LogEvent::Access(event.clone()));
        }
        if let Err(e) = self.sender.try_send(LogEvent::Access(event)) {
            warn!("[logs] access buffer full, dropping event: {e}");
        }
    }

    pub fn log_proxy(&self, event: ProxyEvent) {
        if let Some(loki) = &self.loki {
            loki.send(LogEvent::Proxy(Box::new(event.clone())));
        }
        if let Err(e) = self.sender.try_send(LogEvent::Proxy(Box::new(event))) {
            warn!("[logs] proxy buffer full, dropping event: {e}");
        }
    }

    pub fn log_audit(&self, event: AuditEvent) {
        if let Some(loki) = &self.loki {
            loki.send(LogEvent::Audit(event.clone()));
        }
        if let Err(e) = self.sender.try_send(LogEvent::Audit(event)) {
            warn!("[logs] audit buffer full, dropping event: {e}");
        }
    }

    pub async fn total_requests(&self, start_ts: i64, end_ts: i64) -> u64 {
        self.analytics.total_requests(start_ts, end_ts).await
    }

    pub async fn total_tokens(&self, start_ts: i64, end_ts: i64) -> u64 {
        self.analytics.total_tokens(start_ts, end_ts).await
    }

    pub async fn requests(&self, start_ts: i64, end_ts: i64) -> Vec<BucketEntry> {
        self.analytics.requests(start_ts, end_ts).await
    }

    pub async fn tokens(&self, start_ts: i64, end_ts: i64) -> Vec<BucketEntry> {
        self.analytics.tokens(start_ts, end_ts).await
    }

    pub async fn model_dist(&self, start_ts: i64, end_ts: i64) -> Vec<ModelDistEntry> {
        self.analytics.model_dist(start_ts, end_ts).await
    }

    pub async fn token_dist(&self, start_ts: i64, end_ts: i64) -> Vec<TokenDistEntry> {
        self.analytics.token_dist(start_ts, end_ts).await
    }

    pub async fn query_proxy_logs(&self, params: ProxyLogQueryParams) -> ProxyLogQueryResult {
        self.analytics.query_proxy_logs(params).await
    }

    pub fn shutdown(&self) {
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let _ = sender.send(LogEvent::Shutdown);
        });

        if let Ok(mut guard) = self.worker_handle.lock()
            && let Some(handle) = guard.take()
        {
            let _ = handle.join();
        }
        if let Some(loki) = &self.loki {
            loki.shutdown();
        }
        self.analytics.shutdown();
    }
}

fn worker_loop(
    receiver: mpsc::Receiver<LogEvent>,
    conn: Connection,
    flush_batch: u64,
    flush_interval: Duration,
    retention_every: u64,
    retention_days: u64,
) -> Result<()> {
    let mut buffer: Vec<LogEvent> = Vec::with_capacity(flush_batch as usize);
    let mut flush_count = 0u64;

    loop {
        let mut shutdown = false;

        match receiver.recv_timeout(flush_interval) {
            Ok(LogEvent::Shutdown) => shutdown = true,
            Ok(event) => buffer.push(event),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !buffer.is_empty() {
                    flush_buffer(
                        &conn,
                        &mut buffer,
                        &mut flush_count,
                        retention_every,
                        retention_days,
                    );
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

        if (buffer.len() as u64) >= flush_batch {
            flush_buffer(
                &conn,
                &mut buffer,
                &mut flush_count,
                retention_every,
                retention_days,
            );
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

fn flush_buffer(
    conn: &Connection,
    buffer: &mut Vec<LogEvent>,
    flush_count: &mut u64,
    retention_every: u64,
    retention_days: u64,
) {
    if flush_events(conn, buffer) {
        *flush_count += 1;
        if retention_every > 0 && flush_count.is_multiple_of(retention_every) {
            let _ = conn.execute_batch("CHECKPOINT");
            cleanup_expired(conn, retention_days);
        }
    } else {
        warn!("[logs] flush failed, dropping {} events", buffer.len());
    }
    buffer.clear();
}

fn flush_events(conn: &Connection, events: &[LogEvent]) -> bool {
    let mut access = Vec::new();
    let mut proxy = Vec::new();
    let mut audit = Vec::new();
    for event in events {
        match event {
            LogEvent::Access(e) => access.push(e),
            LogEvent::Proxy(e) => proxy.push(e.as_ref()),
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
        "INSERT INTO access_log (timestamp, request_id, method, path, status, latency_ms, client_ip)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    for e in events {
        stmt.execute(params![
            e.timestamp.naive_utc(),
            e.request_id,
            e.method,
            e.path,
            e.status,
            e.latency_ms,
            e.client_ip,
        ])?;
    }
    Ok(())
}

fn flush_proxies(conn: &Connection, events: &[&ProxyEvent]) -> Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    let mut stmt = conn.prepare_cached(
        "INSERT INTO proxy_log (timestamp, request_id, api_key_name, model_name, provider_name,
         prompt_tokens, completion_tokens, total_tokens, cached_tokens, latency_ms, status,
         endpoint, is_streaming, time_to_first_token_ms, upstream_model, provider_type,
         response_body_size, error_message, client_ip)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                 ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
    )?;
    for e in events {
        stmt.execute(params![
            e.timestamp.naive_utc(),
            e.request_id,
            e.api_key_name,
            e.model_name,
            e.provider_name,
            e.prompt_tokens,
            e.completion_tokens,
            e.total_tokens,
            e.cached_tokens,
            e.latency_ms,
            e.status,
            e.endpoint,
            e.is_streaming,
            e.time_to_first_token_ms,
            e.upstream_model,
            e.provider_type,
            e.response_body_size,
            e.error_message,
            e.client_ip,
        ])?;
    }
    Ok(())
}

fn flush_audits(conn: &Connection, events: &[&AuditEvent]) -> Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    let mut stmt = conn.prepare_cached(
        "INSERT INTO audit_log (timestamp, request_id, action, resource, resource_id, detail)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for e in events {
        stmt.execute(params![
            e.timestamp.naive_utc(),
            e.request_id,
            e.action,
            e.resource,
            e.resource_id,
            e.detail,
        ])?;
    }
    Ok(())
}

fn cleanup_expired(conn: &Connection, retention_days: u64) {
    let cutoff = (Utc::now() - chrono::Duration::days(retention_days as i64)).naive_utc();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{
        make_access_event, make_audit_event, make_proxy_event, test_config_fast_logs,
    };
    use std::time::{Duration, Instant};

    fn temp_log_manager(
        flush_batch: u64,
        flush_interval_secs: u64,
        channel_cap: u64,
        retention_every: u64,
        retention_days: u64,
    ) -> (LogManager, tempfile::TempDir) {
        let dir = tempfile::TempDir::new().unwrap();
        let mut config = test_config_fast_logs(
            dir.path().join("test.db").to_str().unwrap(),
            dir.path().join("logs.duckdb").to_str().unwrap(),
        );
        config.log.flush_batch = flush_batch;
        config.log.flush_interval_secs = flush_interval_secs;
        config.log.channel_cap = channel_cap;
        config.log.retention_every = retention_every;
        config.log.retention_days = retention_days;
        let manager = LogManager::new(&config.log).unwrap();
        (manager, dir)
    }

    #[tokio::test]
    async fn log_access_writes_row_with_fields() {
        let (manager, dir) = temp_log_manager(1, 1, 10000, u64::MAX, 30);
        manager.log_access(make_access_event("/api/providers", 200));
        // shutdown joins the worker, so the flush is guaranteed complete.
        manager.shutdown();
        let conn = Connection::open(dir.path().join("logs.duckdb")).unwrap();
        let (method, path_col, status): (String, String, i32) = conn
            .query_row("SELECT method, path, status FROM access_log", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap();
        assert_eq!(method, "GET");
        assert_eq!(path_col, "/api/providers");
        assert_eq!(status, 200);
    }

    #[tokio::test]
    async fn log_proxy_writes_row_with_fields() {
        let (manager, dir) = temp_log_manager(1, 1, 10000, u64::MAX, 30);
        manager.log_proxy(make_proxy_event("gpt-4", "success", 300));
        // shutdown joins the worker, so the flush is guaranteed complete.
        manager.shutdown();
        let conn = Connection::open(dir.path().join("logs.duckdb")).unwrap();
        let (model, status, total_tokens, is_streaming): (String, String, i64, bool) = conn
            .query_row(
                "SELECT model_name, status, total_tokens, is_streaming FROM proxy_log",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(model, "gpt-4");
        assert_eq!(status, "success");
        assert_eq!(total_tokens, 300);
        assert!(!is_streaming);
    }

    #[tokio::test]
    async fn log_audit_writes_row_with_fields() {
        let (manager, dir) = temp_log_manager(1, 1, 10000, u64::MAX, 30);
        manager.log_audit(make_audit_event("delete"));
        // shutdown joins the worker, so the flush is guaranteed complete.
        manager.shutdown();
        let conn = Connection::open(dir.path().join("logs.duckdb")).unwrap();
        let (action, resource): (String, String) = conn
            .query_row("SELECT action, resource FROM audit_log", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(action, "delete");
        assert_eq!(resource, "api_key");
    }

    #[tokio::test]
    async fn query_returns_written_proxy_rows() {
        let (manager, _dir) = temp_log_manager(1, 1, 10000, u64::MAX, 30);
        manager.log_proxy(make_proxy_event("gpt-4", "success", 300));
        let now = Utc::now().timestamp();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let count = manager.total_requests(now - 3600, now + 3600).await;
            if count >= 1 {
                break;
            }
            if Instant::now() > deadline {
                panic!("timed out waiting for proxy row via analytics query");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let dist = manager.model_dist(now - 3600, now + 3600).await;
        assert_eq!(dist.len(), 1);
        assert_eq!(dist[0].model, "gpt-4");
        assert_eq!(dist[0].count, 1);
        manager.shutdown();
    }

    #[test]
    fn shutdown_flushes_pending_events() {
        let (manager, dir) = temp_log_manager(100, 3600, 10000, u64::MAX, 30);
        // Nothing flushes until shutdown: batch not full, interval not elapsed.
        for i in 0..3 {
            manager.log_proxy(make_proxy_event(&format!("m{i}"), "success", 10));
        }
        manager.shutdown();
        let path = dir.path().join("logs.duckdb");
        let conn = Connection::open(&path).unwrap();
        let count: u64 = conn
            .query_row("SELECT COUNT(*) FROM proxy_log", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn shutdown_is_idempotent() {
        let (manager, _dir) = temp_log_manager(1, 1, 10000, u64::MAX, 30);
        manager.shutdown();
        manager.shutdown();
    }

    #[tokio::test]
    async fn retention_cleanup_deletes_old_rows_keeps_fresh() {
        let (manager, dir) = temp_log_manager(1, 1, 10000, 1, 0);
        let mut old = make_proxy_event("gpt-4", "success", 10);
        old.timestamp = Utc::now() - chrono::Duration::days(2);
        manager.log_proxy(old);
        // shutdown joins the worker, so the flush (and the retention cleanup
        // that follows every flush with retention_every=1) is guaranteed done.
        manager.shutdown();
        let conn = Connection::open(dir.path().join("logs.duckdb")).unwrap();
        let count = |conn: &Connection| -> u64 {
            conn.query_row("SELECT COUNT(*) FROM proxy_log", [], |row| row.get(0))
                .unwrap()
        };
        // The worker's retention cleanup removed the 2-day-old row.
        assert_eq!(count(&conn), 0);

        // The cleanup window logic itself: a fresh row survives a 30-day
        // retention window and is removed by a 0-day one.
        conn.execute(
            "INSERT INTO proxy_log
             (timestamp, request_id, model_name, provider_name, latency_ms, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                Utc::now().naive_utc(),
                "req-1",
                "gpt-4",
                "openai",
                10i64,
                "success",
            ],
        )
        .unwrap();
        cleanup_expired(&conn, 30);
        assert_eq!(count(&conn), 1);
        cleanup_expired(&conn, 0);
        assert_eq!(count(&conn), 0);
    }

    #[tokio::test]
    async fn full_channel_drops_events_without_panic() {
        let (manager, dir) = temp_log_manager(100, 3600, 1, u64::MAX, 30);
        // Let the worker park on recv_timeout, leaving the channel empty.
        std::thread::sleep(Duration::from_millis(100));
        for i in 0..5 {
            manager.log_proxy(make_proxy_event(&format!("m{i}"), "success", 10));
        }
        manager.shutdown();
        let path = dir.path().join("logs.duckdb");
        let conn = Connection::open(&path).unwrap();
        let count: u64 = conn
            .query_row("SELECT COUNT(*) FROM proxy_log", [], |row| row.get(0))
            .unwrap();
        // The channel holds at most 1 event. Whether the worker drains every
        // send or some sends hit a full channel and get dropped depends on
        // thread scheduling, so assert the invariants that actually matter:
        // shutdown flushes at least the buffered event without panicking,
        // and events are never duplicated.
        assert!(
            count >= 1,
            "shutdown must flush at least the buffered event"
        );
        assert!(count <= 5, "events must not be duplicated");
    }
}
