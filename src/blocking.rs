use crate::error::BlockingError;
use tokio::time::{Duration, timeout};

/// Run a blocking operation (DB IO, bcrypt hashing) on a dedicated blocking
/// thread so it does not stall the tokio async executor.
///
/// `Database` is backed by SQLite via `Arc<Mutex<Connection>>`. All database
/// operations must go through this helper because the Mutex must be acquired
/// on a blocking thread. bcrypt is intentionally CPU-bound (~80-250ms).
/// Calling either directly on a tokio worker thread stalls every other task
/// on that thread (including in-flight proxy streams).
///
/// Returns `Err(BlockingError::Timeout)` if the task does not complete within
/// 30 seconds, or `Err(BlockingError::Join)` if the task panicked.
pub async fn run_blocking<T, F>(f: F) -> Result<T, BlockingError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    timeout(Duration::from_secs(30), tokio::task::spawn_blocking(f))
        .await
        .map_err(|_| BlockingError::Timeout)?
        .map_err(BlockingError::Join)
}
