//! Cancels a DuckDB operation that runs past a deadline.
//!
//! `Connection` is not `Sync`, so it cannot be handed to a watchdog thread;
//! `Connection::interrupt_handle()` hands out an `Arc<InterruptHandle>` that
//! can be. Anything that calls into DuckDB on a worker thread goes through
//! [`QueryWatchdog::run`], which arms a deadline the watchdog enforces.
//!
//! This is a backstop, not an expected path: measured on 2M rows, the
//! analytics aggregates take ~10ms and a retention DELETE ~90ms. It exists so
//! a pathological scan cannot pin a worker and, with it, process shutdown -
//! `join()` is the only way to wait for a worker and it has no timeout.

use duckdb::InterruptHandle;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tracing::warn;

/// Sentinel for "no operation in flight".
const IDLE: u64 = u64::MAX;

/// Milliseconds since the first call, so deadlines fit in a `u64`.
fn now_ms() -> u64 {
    static BASE: OnceLock<Instant> = OnceLock::new();
    let base = *BASE.get_or_init(Instant::now);
    base.elapsed().as_millis() as u64
}

pub struct QueryWatchdog {
    deadline: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    timeout_ms: u64,
    thread: Option<JoinHandle<()>>,
}

impl QueryWatchdog {
    /// Start a watchdog for one worker's connection.
    ///
    /// `label` names the worker in the log line written when it fires.
    pub fn spawn(interrupt: Arc<InterruptHandle>, label: &'static str, timeout: Duration) -> Self {
        let deadline = Arc::new(AtomicU64::new(IDLE));
        let stop = Arc::new(AtomicBool::new(false));
        // Check often enough to fire close to the deadline without spinning.
        let poll = (timeout / 4).clamp(Duration::from_millis(10), Duration::from_secs(1));
        let timeout_ms = timeout.as_millis() as u64;

        let watchdog_deadline = deadline.clone();
        let watchdog_stop = stop.clone();
        let thread = thread::spawn(move || {
            loop {
                if watchdog_stop.load(Ordering::Acquire) {
                    return;
                }
                let due = watchdog_deadline.load(Ordering::Acquire);
                if due != IDLE && now_ms() >= due {
                    warn!(
                        "[{}] DuckDB operation exceeded {:?}, interrupting it",
                        label, timeout
                    );
                    interrupt.interrupt();
                    // Disarm: the interrupted call returns on its own, and
                    // firing again would just log repeatedly.
                    watchdog_deadline.store(IDLE, Ordering::Release);
                }
                thread::sleep(poll);
            }
        });

        Self {
            deadline,
            stop,
            timeout_ms,
            thread: Some(thread),
        }
    }

    /// Run `f` under the deadline, interrupting it if it overruns.
    pub fn run<T>(&self, f: impl FnOnce() -> T) -> T {
        self.deadline
            .store(now_ms().saturating_add(self.timeout_ms), Ordering::Release);
        let result = f();
        self.deadline.store(IDLE, Ordering::Release);
        result
    }

    /// Retire the watchdog thread. Called when the worker exits.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use duckdb::Connection;

    fn conn_with_counter(dir: &std::path::Path) -> Connection {
        let conn = Connection::open(dir.join("w.duckdb").to_str().unwrap()).unwrap();
        conn.execute_batch("CREATE TABLE IF NOT EXISTS t (i BIGINT);")
            .unwrap();
        conn
    }

    /// A bulk insert of this size takes tens of seconds on its own, so it is a
    /// stand-in for the pathological operation the watchdog exists to stop.
    const SLOW_INSERT: &str = "INSERT INTO t SELECT i FROM range(5000000) r(i);";

    #[test]
    fn watchdog_interrupts_an_operation_that_overruns() {
        let dir = tempfile::tempdir().unwrap();
        let conn = conn_with_counter(dir.path());
        let watchdog =
            QueryWatchdog::spawn(conn.interrupt_handle(), "test", Duration::from_millis(300));

        let start = Instant::now();
        let result = watchdog.run(|| conn.execute_batch(SLOW_INSERT));
        let elapsed = start.elapsed();
        watchdog.stop();

        assert!(
            elapsed < Duration::from_secs(5),
            "watchdog did not interrupt; the insert ran for {elapsed:?}"
        );
        assert!(result.is_err(), "an interrupted query must report an error");
    }

    #[test]
    fn watchdog_leaves_a_fast_operation_alone() {
        let dir = tempfile::tempdir().unwrap();
        let conn = conn_with_counter(dir.path());
        let watchdog =
            QueryWatchdog::spawn(conn.interrupt_handle(), "test", Duration::from_secs(30));

        // Long enough that a false positive would show up as an error.
        let result = watchdog.run(|| conn.execute_batch("INSERT INTO t VALUES (1), (2), (3);"));
        watchdog.stop();
        assert!(result.is_ok(), "a fast query must not be interrupted");
    }
}
