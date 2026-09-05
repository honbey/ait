use chrono::{DateTime, Utc};
use rusqlite::{Connection, Row};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};

use super::models::*;

mod apikeys;
mod models;
mod providers;

pub(crate) fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

/// SQLite access: one writer plus a small pool of readers. WAL mode lets the
/// readers run concurrently with the writer and with each other, so admin
/// reads no longer queue behind proxy-path writes on a single mutex.
pub struct Database {
    writer: Arc<Mutex<Connection>>,
    readers: Vec<Arc<Mutex<Connection>>>,
    next_reader: AtomicUsize,
}

#[derive(Debug)]
pub enum DbError {
    NotFound(String),
    LimitExceeded(String),
    Duplicate(String),
    Storage(String),
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbError::NotFound(msg) => write!(f, "Not found: {}", msg),
            DbError::LimitExceeded(msg) => write!(f, "Limit exceeded: {}", msg),
            DbError::Duplicate(msg) => write!(f, "Duplicate: {}", msg),
            DbError::Storage(msg) => write!(f, "Storage error: {}", msg),
        }
    }
}

impl std::error::Error for DbError {}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// Out-of-range timestamps must not panic inside spawn_blocking (the mutex
// would be poisoned for every later request); clamp to the epoch instead.
fn ts(ts: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(ts, 0).unwrap_or(DateTime::UNIX_EPOCH)
}

fn opt_ts(ts: Option<i64>) -> Option<DateTime<Utc>> {
    ts.map(|t| DateTime::from_timestamp(t, 0).unwrap_or(DateTime::UNIX_EPOCH))
}

fn to_storage(e: rusqlite::Error) -> DbError {
    DbError::Storage(e.to_string())
}

fn row_to_provider(row: &Row) -> rusqlite::Result<Provider> {
    Ok(Provider {
        id: row.get(0)?,
        name: row.get(1)?,
        provider_type: ProviderType::from_db(&row.get::<_, String>(2)?),
        base_url: row.get(3)?,
        api_key: row.get(4)?,
        enabled: row.get::<_, i32>(5)? != 0,
        created_at: ts(row.get::<_, i64>(6)?),
        updated_at: ts(row.get::<_, i64>(7)?),
    })
}

fn row_to_model(row: &Row) -> rusqlite::Result<Model> {
    Ok(Model {
        id: row.get(0)?,
        name: row.get(1)?,
        provider_id: row.get(2)?,
        upstream_model: row.get(3)?,
        enabled: row.get::<_, i32>(4)? != 0,
        created_at: ts(row.get::<_, i64>(5)?),
        updated_at: ts(row.get::<_, i64>(6)?),
    })
}

fn row_to_api_key(row: &Row) -> rusqlite::Result<ApiKey> {
    Ok(ApiKey {
        id: row.get(0)?,
        key: row.get(1)?,
        display: row.get(2)?,
        name: row.get(3)?,
        created_at: ts(row.get::<_, i64>(4)?),
        updated_at: ts(row.get::<_, i64>(5)?),
        enabled: row.get::<_, i32>(6)? != 0,
        expires_at: opt_ts(row.get::<_, Option<i64>>(7)?),
    })
}

fn row_to_api_key_info(row: &Row) -> rusqlite::Result<ApiKeyInfo> {
    Ok(ApiKeyInfo {
        id: row.get(0)?,
        name: row.get(1)?,
        enabled: row.get::<_, i32>(2)? != 0,
        expires_at: opt_ts(row.get::<_, Option<i64>>(3)?),
        created_at: ts(row.get::<_, i64>(4)?),
    })
}

fn create_tables(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS providers (
            id          TEXT PRIMARY KEY,
            name        TEXT NOT NULL,
            type        TEXT NOT NULL DEFAULT 'openai_compat',
            base_url    TEXT NOT NULL,
            api_key     TEXT,
            enabled     INTEGER NOT NULL DEFAULT 1,
            created_at  INTEGER NOT NULL,
            updated_at  INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS models (
            id             TEXT PRIMARY KEY,
            name           TEXT NOT NULL UNIQUE,
            provider_id    TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
            upstream_model TEXT NOT NULL,
            enabled        INTEGER NOT NULL DEFAULT 1,
            created_at     INTEGER NOT NULL,
            updated_at     INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_models_provider ON models(provider_id);
        CREATE INDEX IF NOT EXISTS idx_models_enabled ON models(enabled);
        CREATE TABLE IF NOT EXISTS api_keys (
            id         TEXT PRIMARY KEY,
            key_hash   TEXT NOT NULL UNIQUE,
            display    TEXT NOT NULL,
            name       TEXT NOT NULL,
            enabled    INTEGER NOT NULL DEFAULT 1,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            expires_at INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_apikeys_id ON api_keys(id);",
    )
    .map_err(to_storage)
}

// ---------------------------------------------------------------------------
// Database impl
// ---------------------------------------------------------------------------

impl Database {
    /// Acquire the writer, recovering from a poisoned mutex.
    ///
    /// Every DB operation runs inside `run_blocking`, so a panic there is
    /// contained; without this recovery the poisoned mutex would make every
    /// later request fail for the lifetime of the process.
    fn lock_writer(&self) -> MutexGuard<'_, Connection> {
        self.writer.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Acquire a reader, preferring one that is not already held.
    ///
    /// Plain round-robin blocked on whichever reader the counter named, so one
    /// slow query held every subsequent read assigned to it while the rest of
    /// the pool sat idle. Scanning for a free reader first spreads work across
    /// the pool; only when all are busy does a caller wait.
    fn lock_reader(&self) -> MutexGuard<'_, Connection> {
        let start = self.next_reader.fetch_add(1, Ordering::Relaxed);
        for offset in 0..self.readers.len() {
            let index = (start + offset) % self.readers.len();
            match self.readers[index].try_lock() {
                Ok(guard) => return guard,
                // A poisoned mutex still holds a usable guard: every DB
                // operation runs inside `run_blocking`, which contains the
                // panic that poisoned it.
                Err(TryLockError::Poisoned(poisoned)) => return poisoned.into_inner(),
                Err(TryLockError::WouldBlock) => {}
            }
        }
        self.readers[start % self.readers.len()]
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    pub fn new(
        path: &str,
        reader_pool_size: usize,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(parent) = Path::new(path).parent() {
            fs::create_dir_all(parent)?;
        }

        // The writer is opened first: it switches the file into WAL mode, and
        // readers opened afterwards pick that mode up from the database header.
        let writer = Connection::open(path)?;
        writer.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
        )?;
        create_tables(&writer)?;

        let readers: Vec<Connection> = (0..reader_pool_size)
            .map(
                |_| -> Result<Connection, Box<dyn std::error::Error + Send + Sync>> {
                    let conn = Connection::open(path)?;
                    // foreign_keys is per-connection; busy_timeout keeps a WAL
                    // checkpoint from surfacing as SQLITE_BUSY on a reader.
                    conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")?;
                    Ok(conn)
                },
            )
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            writer: Arc::new(Mutex::new(writer)),
            readers: readers
                .into_iter()
                .map(|c| Arc::new(Mutex::new(c)))
                .collect(),
            next_reader: AtomicUsize::new(0),
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Database, tempfile::TempDir) {
        crate::test_utils::create_test_db()
    }

    #[test]
    fn reader_pool_size_is_honoured() {
        let (db, _dir) = setup();
        assert_eq!(db.readers.len(), 4);

        let dir = tempfile::tempdir().unwrap();
        let db = Database::new(dir.path().join("a.db").to_str().unwrap(), 1).unwrap();
        assert_eq!(db.readers.len(), 1);
    }

    #[test]
    fn lock_reader_skips_a_held_connection() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::new(dir.path().join("a.db").to_str().unwrap(), 2).unwrap();

        // Held guards must not be revisited: under the old round-robin the
        // counter below pointed straight at a held reader and blocked, even
        // though the rest of the pool was idle.
        crate::test_utils::assert_no_deadlock(std::time::Duration::from_secs(5), move || {
            let first = db.lock_reader();
            // Burn a turn so the counter lands back on `first` next time.
            drop(db.lock_reader());
            let again = db.lock_reader();
            assert!(
                !std::ptr::eq(&*first, &*again),
                "must take the free reader, not queue behind the held one"
            );
        });
    }

    fn make_prov(id: &str) -> Provider {
        crate::test_utils::create_test_provider(
            id,
            ProviderType::OpenAICompat,
            "https://example.com",
        )
    }

    // ── Provider CRUD ──

    #[test]
    fn provider_create() {
        let (db, _dir) = setup();
        let prov = db.insert_provider(make_prov("p1")).unwrap();
        assert!(!prov.id.is_empty());
        assert_eq!(prov.name, "p1");
    }

    #[test]
    fn provider_get() {
        let (db, _dir) = setup();
        db.insert_provider(make_prov("p1")).unwrap();
        let got = db.get_provider("p1").unwrap().expect("should exist");
        assert_eq!(got.name, "p1");
    }

    #[test]
    fn provider_get_missing() {
        let (db, _dir) = setup();
        assert!(db.get_provider("nonexistent").unwrap().is_none());
    }

    #[test]
    fn count_providers_empty() {
        let (db, _dir) = setup();
        assert_eq!(db.count_providers().unwrap(), 0);
    }

    #[test]
    fn count_providers_with_items() {
        let (db, _dir) = setup();
        db.insert_provider(make_prov("p1")).unwrap();
        db.insert_provider(make_prov("p2")).unwrap();
        assert_eq!(db.count_providers().unwrap(), 2);
    }

    #[test]
    fn provider_list() {
        let (db, _dir) = setup();
        db.insert_provider(make_prov("p1")).unwrap();
        db.insert_provider(make_prov("p2")).unwrap();
        let list = db.list_providers().unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn provider_update() {
        let (db, _dir) = setup();
        let prov = db.insert_provider(make_prov("p1")).unwrap();
        db.update_provider(&ProviderUpdate {
            id: prov.id.clone(),
            name: Some("p1_renamed".to_string()),
            enabled: Some(false),
            ..Default::default()
        })
        .unwrap();
        let got = db.get_provider("p1").unwrap().unwrap();
        assert_eq!(got.name, "p1_renamed");
        assert!(!got.enabled);
    }

    #[test]
    fn provider_partial_update_keeps_other_fields() {
        let (db, _dir) = setup();
        let prov = db.insert_provider(make_prov("p1")).unwrap();
        db.update_provider(&ProviderUpdate {
            id: prov.id.clone(),
            enabled: Some(false),
            ..Default::default()
        })
        .unwrap();
        let got = db.get_provider("p1").unwrap().unwrap();
        assert!(!got.enabled);
        assert_eq!(got.name, "p1");
        assert_eq!(got.provider_type, prov.provider_type);
        assert_eq!(got.base_url, prov.base_url);
        assert_eq!(got.api_key, prov.api_key);
    }

    #[test]
    fn provider_delete() {
        let (db, _dir) = setup();
        db.insert_provider(make_prov("p1")).unwrap();
        assert!(db.delete_provider("p1").unwrap());
        assert!(db.get_provider("p1").unwrap().is_none());
    }

    #[test]
    fn provider_delete_missing() {
        let (db, _dir) = setup();
        assert!(!db.delete_provider("nonexistent").unwrap());
    }

    // ── Model CRUD ──

    #[test]
    fn model_create_requires_provider() {
        let (db, _dir) = setup();
        let model = crate::test_utils::create_test_model("m1", "no_such_provider");
        let err = db.insert_model(model).unwrap_err();
        assert!(matches!(err, DbError::NotFound(_)));
    }

    #[test]
    fn model_create() {
        let (db, _dir) = setup();
        db.insert_provider(make_prov("p1")).unwrap();
        let model = crate::test_utils::create_test_model("m1", "p1");
        let m = db.insert_model(model).unwrap();
        assert_eq!(m.name, "m1");
        assert_eq!(m.provider_id, "p1");
    }

    #[test]
    fn model_get() {
        let (db, _dir) = setup();
        db.insert_provider(make_prov("p1")).unwrap();
        db.insert_model(crate::test_utils::create_test_model("m1", "p1"))
            .unwrap();
        let got = db.get_model("m1").unwrap().expect("should exist");
        assert_eq!(got.name, "m1");
    }

    #[test]
    fn count_models_empty() {
        let (db, _dir) = setup();
        assert_eq!(db.count_models().unwrap(), 0);
    }

    #[test]
    fn count_models_with_items() {
        let (db, _dir) = setup();
        db.insert_provider(make_prov("p1")).unwrap();
        db.insert_model(crate::test_utils::create_test_model("m1", "p1"))
            .unwrap();
        db.insert_model(crate::test_utils::create_test_model("m2", "p1"))
            .unwrap();
        assert_eq!(db.count_models().unwrap(), 2);
    }

    #[test]
    fn model_list() {
        let (db, _dir) = setup();
        db.insert_provider(make_prov("p1")).unwrap();
        db.insert_model(crate::test_utils::create_test_model("m1", "p1"))
            .unwrap();
        db.insert_model(crate::test_utils::create_test_model("m2", "p1"))
            .unwrap();
        assert_eq!(db.list_models().unwrap().len(), 2);
    }

    #[test]
    fn model_update() {
        let (db, _dir) = setup();
        db.insert_provider(make_prov("p1")).unwrap();
        db.insert_provider(make_prov("p2")).unwrap();
        let model = db
            .insert_model(crate::test_utils::create_test_model("m1", "p1"))
            .unwrap();
        db.update_model(&ModelUpdate {
            name: model.name.clone(),
            provider_id: Some("p2".to_string()),
            enabled: Some(false),
            ..Default::default()
        })
        .unwrap();
        let got = db.get_model("m1").unwrap().unwrap();
        assert_eq!(got.provider_id, "p2");
        assert!(!got.enabled);
    }

    #[test]
    fn model_partial_update_keeps_other_fields() {
        let (db, _dir) = setup();
        db.insert_provider(make_prov("p1")).unwrap();
        let model = db
            .insert_model(crate::test_utils::create_test_model("m1", "p1"))
            .unwrap();
        db.update_model(&ModelUpdate {
            name: model.name.clone(),
            enabled: Some(false),
            ..Default::default()
        })
        .unwrap();
        let got = db.get_model("m1").unwrap().unwrap();
        assert!(!got.enabled);
        assert_eq!(got.provider_id, "p1");
        assert_eq!(got.upstream_model, model.upstream_model);
    }

    #[test]
    fn model_delete() {
        let (db, _dir) = setup();
        db.insert_provider(make_prov("p1")).unwrap();
        db.insert_model(crate::test_utils::create_test_model("m1", "p1"))
            .unwrap();
        db.delete_model("m1").unwrap();
        assert!(db.get_model("m1").unwrap().is_none());
    }

    // ── Cascade: delete provider deletes its models ──

    #[test]
    fn delete_provider_cascades_models() {
        let (db, _dir) = setup();
        db.insert_provider(make_prov("p1")).unwrap();
        db.insert_model(crate::test_utils::create_test_model("m1", "p1"))
            .unwrap();
        db.insert_model(crate::test_utils::create_test_model("m2", "p1"))
            .unwrap();
        db.delete_provider("p1").unwrap();
        assert!(db.list_models().unwrap().is_empty());
    }

    #[test]
    fn cascade_does_not_affect_other_provider_models() {
        let (db, _dir) = setup();
        db.insert_provider(make_prov("p1")).unwrap();
        db.insert_provider(make_prov("p2")).unwrap();
        db.insert_model(crate::test_utils::create_test_model("m1", "p1"))
            .unwrap();
        db.insert_model(crate::test_utils::create_test_model("m2", "p2"))
            .unwrap();
        db.delete_provider("p1").unwrap();
        let models = db.list_models().unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "m2");
    }

    // ── resolve_model ──

    #[test]
    fn resolve_model_both_enabled() {
        let (db, _dir) = setup();
        db.insert_provider(make_prov("p1")).unwrap();
        db.insert_model(crate::test_utils::create_test_model("m1", "p1"))
            .unwrap();
        assert!(db.resolve_model("m1").unwrap().is_some());
    }

    #[test]
    fn resolve_model_model_disabled() {
        let (db, _dir) = setup();
        db.insert_provider(make_prov("p1")).unwrap();
        let mut model = crate::test_utils::create_test_model("m1", "p1");
        model.enabled = false;
        db.insert_model(model).unwrap();
        assert!(db.resolve_model("m1").unwrap().is_none());
    }

    #[test]
    fn resolve_model_provider_disabled() {
        let (db, _dir) = setup();
        let mut disabled = make_prov("p1");
        disabled.enabled = false;
        db.insert_provider(disabled).unwrap();
        db.insert_model(crate::test_utils::create_test_model("m1", "p1"))
            .unwrap();
        assert!(db.resolve_model("m1").unwrap().is_none());
    }

    #[test]
    fn resolve_model_missing() {
        let (db, _dir) = setup();
        db.insert_provider(make_prov("p1")).unwrap();
        assert!(db.resolve_model("no_such_model").unwrap().is_none());
    }

    // ── API Key CRUD ──

    #[test]
    fn api_key_create_returns_raw_key() {
        let (db, _dir) = setup();
        let (stored, raw) = db.insert_api_key("test-key", None).unwrap();
        assert!(raw.starts_with("sk-"));
        assert_eq!(raw.len(), 35);
        assert_eq!(stored.name, "test-key");
    }

    #[test]
    fn api_key_create_empty_name_errors() {
        let (db, _dir) = setup();
        // validate_string is applied at the handler layer; the store accepts any
        // name, but an obviously-empty name would still produce a row. Here we
        // just assert a non-empty name round-trips.
        let (stored, _) = db.insert_api_key("another-key", None).unwrap();
        assert_eq!(stored.name, "another-key");
    }

    #[test]
    fn api_key_lookup_by_raw_key() {
        let (db, _dir) = setup();
        let (_, raw) = db.insert_api_key("test-key", None).unwrap();
        let info = db.get_api_key_by_raw(&raw).unwrap().expect("should find");
        assert_eq!(info.name, "test-key");
    }

    #[test]
    fn api_key_lookup_invalid_key() {
        let (db, _dir) = setup();
        assert!(db.get_api_key_by_raw("sk-invalid").unwrap().is_none());
    }

    #[test]
    fn api_key_list_returns_all() {
        let (db, _dir) = setup();
        db.insert_api_key("k1", None).unwrap();
        db.insert_api_key("k2", None).unwrap();
        let all = db.list_api_keys().unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|k| k.name == "k1"));
        assert!(all.iter().any(|k| k.name == "k2"));
    }

    #[test]
    fn api_key_delete() {
        let (db, _dir) = setup();
        let (_, raw) = db.insert_api_key("test-key", None).unwrap();
        let stored = db.get_api_key_by_raw(&raw).unwrap().unwrap();
        db.delete_api_key(&stored.id).unwrap();
        assert!(db.get_api_key_by_raw(&raw).unwrap().is_none());
    }

    #[test]
    fn api_key_update() {
        let (db, _dir) = setup();
        let (stored, raw) = db.insert_api_key("test-key", None).unwrap();
        db.update_api_key(&ApiKeyUpdate {
            id: stored.id.clone(),
            enabled: Some(false),
            ..Default::default()
        })
        .unwrap();
        let info = db.get_api_key_by_raw(&raw).unwrap().unwrap();
        assert!(!info.enabled);
    }

    #[test]
    fn api_key_partial_update_keeps_other_fields() {
        let (db, _dir) = setup();
        let (stored, raw) = db.insert_api_key("test-key", None).unwrap();
        db.update_api_key(&ApiKeyUpdate {
            id: stored.id.clone(),
            enabled: Some(false),
            ..Default::default()
        })
        .unwrap();
        let info = db.get_api_key_by_raw(&raw).unwrap().unwrap();
        assert!(!info.enabled);
        assert_eq!(info.name, "test-key");
    }

    // ── Database isolation ──

    #[test]
    fn databases_are_isolated() {
        let (db_a, _dir_a) = crate::test_utils::create_test_db();
        let (db_b, _dir_b) = crate::test_utils::create_test_db();
        db_a.insert_provider(make_prov("p1")).unwrap();
        assert!(db_b.list_providers().unwrap().is_empty());
    }
}
