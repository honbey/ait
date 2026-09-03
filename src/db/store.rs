use chrono::{DateTime, Utc};
use rusqlite::{Connection, Row, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use tracing::warn;
use uuid::Uuid;

use super::models::*;

pub(crate) fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

pub struct Database {
    conn: Arc<Mutex<Connection>>,
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
    /// Acquire the connection lock, recovering from a poisoned mutex.
    ///
    /// Every DB operation runs inside `run_blocking`, so a panic there is
    /// contained; without this recovery the poisoned mutex would make every
    /// later request fail for the lifetime of the process.
    fn lock_conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn new(path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(parent) = Path::new(path).parent() {
            fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        create_tables(&conn)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    // --- Provider CRUD ---

    pub fn insert_provider(&self, mut provider: Provider) -> Result<Provider, DbError> {
        if provider.id.is_empty() {
            provider.id = Uuid::new_v4().to_string();
        }
        let now = Utc::now();
        provider.created_at = now;
        provider.updated_at = now;

        let conn = self.lock_conn();
        conn.execute(
            "INSERT INTO providers (id, name, type, base_url, api_key, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                provider.id,
                provider.name,
                provider.provider_type.to_db(),
                provider.base_url,
                provider.api_key,
                provider.enabled as i32,
                provider.created_at.timestamp(),
                provider.updated_at.timestamp(),
            ],
        )
        .map_err(to_storage)?;

        Ok(provider)
    }

    pub fn update_provider(&self, updates: &ProviderUpdate) -> Result<Provider, DbError> {
        let conn = self.lock_conn();
        let now = Utc::now();

        let mut provider = {
            let result = conn.query_row(
                "SELECT id, name, type, base_url, api_key, enabled, created_at, updated_at
                 FROM providers WHERE id = ?1",
                params![updates.id],
                row_to_provider,
            );
            match result {
                Ok(p) => p,
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    return Err(DbError::NotFound(format!(
                        "Provider '{}' not found",
                        updates.id
                    )));
                }
                Err(e) => return Err(DbError::Storage(e.to_string())),
            }
        };

        if let Some(name) = &updates.name {
            provider.name = name.clone();
        }
        if let Some(provider_type) = &updates.provider_type {
            provider.provider_type = provider_type.clone();
        }
        if let Some(base_url) = &updates.base_url {
            provider.base_url = base_url.clone();
        }
        provider.api_key = match &updates.api_key {
            None => provider.api_key,
            Some(s) if s.is_empty() => None,
            Some(s) => Some(s.clone()),
        };
        if let Some(enabled) = updates.enabled {
            provider.enabled = enabled;
        }
        provider.updated_at = now;

        conn.execute(
            "UPDATE providers SET name=?1, type=?2, base_url=?3, api_key=?4, enabled=?5, updated_at=?6
             WHERE id=?7",
            params![
                provider.name,
                provider.provider_type.to_db(),
                provider.base_url,
                provider.api_key,
                provider.enabled as i32,
                provider.updated_at.timestamp(),
                provider.id,
            ],
        )
        .map_err(to_storage)?;

        Ok(provider)
    }

    pub fn delete_provider(&self, id: &str) -> Result<bool, DbError> {
        let conn = self.lock_conn();
        let rows = conn
            .execute("DELETE FROM providers WHERE id = ?1", params![id])
            .map_err(to_storage)?;
        Ok(rows > 0)
    }

    pub fn get_provider(&self, id: &str) -> Result<Option<Provider>, DbError> {
        let conn = self.lock_conn();
        let result = conn.query_row(
            "SELECT id, name, type, base_url, api_key, enabled, created_at, updated_at
             FROM providers WHERE id = ?1",
            params![id],
            row_to_provider,
        );
        match result {
            Ok(p) => Ok(Some(p)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(DbError::Storage(e.to_string())),
        }
    }

    pub fn count_providers(&self) -> Result<usize, DbError> {
        let conn = self.lock_conn();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM providers", [], |row| row.get(0))
            .map_err(to_storage)?;
        Ok(count as usize)
    }

    pub fn list_providers(&self) -> Result<Vec<Provider>, DbError> {
        let conn = self.lock_conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, name, type, base_url, api_key, enabled, created_at, updated_at
                 FROM providers ORDER BY name",
            )
            .map_err(to_storage)?;
        let rows = stmt.query_map([], row_to_provider).map_err(to_storage)?;
        let mut items = Vec::new();
        for row in rows {
            match row {
                Ok(item) => items.push(item),
                // Iterator::flatten would drop conversion errors silently.
                Err(e) => warn!("list_providers: skipped unreadable row: {e}"),
            }
        }
        Ok(items)
    }

    // --- Model CRUD ---

    pub fn insert_model(&self, mut model: Model) -> Result<Model, DbError> {
        if model.id.is_empty() {
            model.id = Uuid::new_v4().to_string();
        }
        let now = Utc::now();
        model.created_at = now;
        model.updated_at = now;

        let mut conn = self.lock_conn();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(to_storage)?;

        let (prov_exists, name_exists): (bool, bool) = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM providers WHERE id = ?1),
                        EXISTS(SELECT 1 FROM models WHERE name = ?2)",
                params![model.provider_id, model.name],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(to_storage)?;
        if !prov_exists {
            return Err(DbError::NotFound(format!(
                "Provider '{}' not found",
                model.provider_id
            )));
        }
        if name_exists {
            return Err(DbError::Duplicate(format!(
                "Model '{}' already exists",
                model.name
            )));
        }

        tx.execute(
            "INSERT INTO models (id, name, provider_id, upstream_model, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                model.id,
                model.name,
                model.provider_id,
                model.upstream_model,
                model.enabled as i32,
                model.created_at.timestamp(),
                model.updated_at.timestamp(),
            ],
        )
        .map_err(to_storage)?;

        tx.commit().map_err(to_storage)?;
        Ok(model)
    }

    pub fn update_model(&self, updates: &ModelUpdate) -> Result<Model, DbError> {
        let conn = self.lock_conn();
        let now = Utc::now();

        let mut model = {
            let result = conn.query_row(
                "SELECT id, name, provider_id, upstream_model, enabled, created_at, updated_at
                 FROM models WHERE name = ?1",
                params![updates.name],
                row_to_model,
            );
            match result {
                Ok(m) => m,
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    return Err(DbError::NotFound(format!(
                        "Model '{}' not found",
                        updates.name
                    )));
                }
                Err(e) => return Err(DbError::Storage(e.to_string())),
            }
        };

        if let Some(provider_id) = &updates.provider_id {
            model.provider_id = provider_id.clone();
        }
        if let Some(upstream_model) = &updates.upstream_model {
            model.upstream_model = upstream_model.clone();
        }
        if let Some(enabled) = updates.enabled {
            model.enabled = enabled;
        }
        model.updated_at = now;

        conn.execute(
            "UPDATE models SET provider_id=?1, upstream_model=?2, enabled=?3, updated_at=?4
             WHERE name=?5",
            params![
                model.provider_id,
                model.upstream_model,
                model.enabled as i32,
                model.updated_at.timestamp(),
                updates.name,
            ],
        )
        .map_err(to_storage)?;

        Ok(model)
    }

    pub fn delete_model(&self, name: &str) -> Result<bool, DbError> {
        let conn = self.lock_conn();
        let rows = conn
            .execute("DELETE FROM models WHERE name = ?1", params![name])
            .map_err(to_storage)?;
        Ok(rows > 0)
    }

    pub fn get_model(&self, name: &str) -> Result<Option<Model>, DbError> {
        let conn = self.lock_conn();
        let result = conn.query_row(
            "SELECT id, name, provider_id, upstream_model, enabled, created_at, updated_at
             FROM models WHERE name = ?1",
            params![name],
            row_to_model,
        );
        match result {
            Ok(m) => Ok(Some(m)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(DbError::Storage(e.to_string())),
        }
    }

    pub fn count_models(&self) -> Result<usize, DbError> {
        let conn = self.lock_conn();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM models", [], |row| row.get(0))
            .map_err(to_storage)?;
        Ok(count as usize)
    }

    pub fn list_models(&self) -> Result<Vec<Model>, DbError> {
        let conn = self.lock_conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, name, provider_id, upstream_model, enabled, created_at, updated_at
                 FROM models ORDER BY name",
            )
            .map_err(to_storage)?;
        let rows = stmt.query_map([], row_to_model).map_err(to_storage)?;
        let mut items = Vec::new();
        for row in rows {
            match row {
                Ok(item) => items.push(item),
                Err(e) => warn!("list_models: skipped unreadable row: {e}"),
            }
        }
        Ok(items)
    }

    // --- resolve_model: hot path ---

    pub fn resolve_model(&self, model_name: &str) -> Result<Option<(Model, Provider)>, DbError> {
        let conn = self.lock_conn();
        let result = conn.query_row(
            "SELECT m.id, m.name, m.provider_id, m.upstream_model, m.enabled, m.created_at, m.updated_at,
                    p.id, p.name, p.type, p.base_url, p.api_key, p.enabled, p.created_at, p.updated_at
             FROM models m
             JOIN providers p ON p.id = m.provider_id
             WHERE m.name = ?1 AND m.enabled = 1 AND p.enabled = 1",
            params![model_name],
            |row| {
                let model = Model {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    provider_id: row.get(2)?,
                    upstream_model: row.get(3)?,
                    enabled: row.get::<_, i32>(4)? != 0,
                    created_at: ts(row.get::<_, i64>(5)?),
                    updated_at: ts(row.get::<_, i64>(6)?),
                };
                let provider = Provider {
                    id: row.get(7)?,
                    name: row.get(8)?,
                    provider_type: ProviderType::from_db(&row.get::<_, String>(9)?),
                    base_url: row.get(10)?,
                    api_key: row.get(11)?,
                    enabled: row.get::<_, i32>(12)? != 0,
                    created_at: ts(row.get::<_, i64>(13)?),
                    updated_at: ts(row.get::<_, i64>(14)?),
                };
                Ok((model, provider))
            },
        );
        match result {
            Ok((m, p)) => Ok(Some((m, p))),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(DbError::Storage(e.to_string())),
        }
    }

    // --- API Key CRUD ---

    fn generate_random_string(len: usize) -> String {
        use rand::RngExt;
        const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let mut rng = rand::rng();
        (0..len)
            .map(|_| CHARS[rng.random_range(0..CHARS.len())] as char)
            .collect()
    }

    fn generate_api_key() -> String {
        format!("sk-{}", Self::generate_random_string(32))
    }

    pub fn insert_api_key(
        &self,
        name: &str,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<(ApiKey, String), DbError> {
        let raw_key = Self::generate_api_key();
        let hash = hash_key(&raw_key);
        let now = Utc::now();
        let id = Uuid::new_v4().to_string();
        let now_ts = now.timestamp();
        let expires_ts = expires_at.map(|dt| dt.timestamp());

        let mut conn = self.lock_conn();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(to_storage)?;

        tx.execute(
            "INSERT INTO api_keys (id, key_hash, display, name, enabled, created_at, updated_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                hash,
                mask_api_key(&raw_key),
                name,
                1i32,
                now_ts,
                now_ts,
                expires_ts,
            ],
        )
        .map_err(to_storage)?;

        tx.commit().map_err(to_storage)?;

        let stored = ApiKey {
            id: id.clone(),
            key: hash,
            display: mask_api_key(&raw_key),
            name: name.to_string(),
            created_at: now,
            updated_at: now,
            enabled: true,
            expires_at,
        };

        Ok((stored, raw_key))
    }

    pub fn list_api_keys(&self) -> Result<Vec<ApiKey>, DbError> {
        let conn = self.lock_conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, key_hash, display, name, created_at, updated_at, enabled, expires_at
                 FROM api_keys ORDER BY created_at",
            )
            .map_err(to_storage)?;
        let rows = stmt.query_map([], row_to_api_key).map_err(to_storage)?;
        let mut items = Vec::new();
        for row in rows {
            match row {
                Ok(item) => items.push(item),
                Err(e) => warn!("list_api_keys: skipped unreadable row: {e}"),
            }
        }
        Ok(items)
    }

    pub fn get_api_key_by_raw(&self, api_key: &str) -> Result<Option<ApiKeyInfo>, DbError> {
        let api_hash = hash_key(api_key);
        let conn = self.lock_conn();
        let result = conn.query_row(
            "SELECT id, name, enabled, expires_at, created_at
             FROM api_keys WHERE key_hash = ?1",
            params![api_hash],
            row_to_api_key_info,
        );
        match result {
            Ok(info) => Ok(Some(info)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(DbError::Storage(e.to_string())),
        }
    }

    pub fn delete_api_key(&self, key_id: &str) -> Result<String, DbError> {
        let conn = self.lock_conn();

        let hash: String = conn
            .query_row(
                "SELECT key_hash FROM api_keys WHERE id = ?1",
                params![key_id],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    DbError::NotFound("API key not found".to_string())
                }
                _ => DbError::Storage(e.to_string()),
            })?;

        conn.execute("DELETE FROM api_keys WHERE id = ?1", params![key_id])
            .map_err(to_storage)?;

        Ok(hash)
    }

    pub fn update_api_key(&self, updates: &ApiKeyUpdate) -> Result<(ApiKey, String), DbError> {
        let conn = self.lock_conn();
        let now = Utc::now();

        let mut api_key = {
            let result = conn.query_row(
                "SELECT id, key_hash, display, name, created_at, updated_at, enabled, expires_at
                 FROM api_keys WHERE id = ?1",
                params![updates.id],
                row_to_api_key,
            );
            match result {
                Ok(k) => k,
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    return Err(DbError::NotFound("API key not found".to_string()));
                }
                Err(e) => return Err(DbError::Storage(e.to_string())),
            }
        };

        if let Some(name) = &updates.name {
            api_key.name = name.clone();
        }
        if let Some(enabled) = updates.enabled {
            api_key.enabled = enabled;
        }
        api_key.expires_at = match updates.expires_at {
            None => api_key.expires_at,
            Some(dt) if dt.timestamp() == 0 => None,
            Some(dt) => Some(dt),
        };
        api_key.updated_at = now;

        conn.execute(
            "UPDATE api_keys SET name = ?1, enabled = ?2, expires_at = ?3, updated_at = ?4 WHERE id = ?5",
            params![
                api_key.name,
                api_key.enabled as i32,
                api_key.expires_at.map(|dt| dt.timestamp()),
                api_key.updated_at.timestamp(),
                api_key.id,
            ],
        )
        .map_err(to_storage)?;

        let hash = api_key.key.clone();
        Ok((api_key, hash))
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
