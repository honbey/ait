/// Run a blocking operation (DB IO, bcrypt hashing) on a dedicated blocking
/// thread so it does not stall the tokio async executor.
///
/// `Database` is backed by SQLite via `Arc<Mutex<Connection>>`. All database
/// operations must go through this helper because the Mutex must be acquired
/// on a blocking thread. bcrypt is intentionally CPU-bound (~80-250ms).
/// Calling either directly on a tokio worker thread stalls every other task
/// on that thread (including in-flight proxy streams).
///
/// Returns `Err(JoinError)` if the blocking task panicked. Callers should
/// convert this to an appropriate error response (e.g., `internal_error`).
pub async fn run_blocking<T, F>(f: F) -> Result<T, tokio::task::JoinError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    tokio::task::spawn_blocking(f).await
}
