/// Run a blocking operation (DB IO, bcrypt hashing) on a dedicated blocking
/// thread so it does not stall the tokio async executor.
///
/// `Database` is backed by RocksDB via an `Arc`, which is `Send + Sync`, and
/// bcrypt is intentionally CPU-bound (~80-250ms). Calling either directly on a
/// tokio worker thread stalls every other task on that thread (including
/// in-flight proxy streams). This helper offloads them to the blocking pool.
///
/// Panics from the blocking task propagate to the caller (fail-fast), which is
/// acceptable since `Database` methods return `Result`s and do not panic.
pub async fn run_blocking<T, F>(f: F) -> T
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .expect("blocking task panicked")
}
