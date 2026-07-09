use chrono::{DateTime, Utc};
use rusqlite::{Connection, Row, params};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use super::models::*;

pub(crate) fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

pub struct Database {
    conn: Arc<Mutex<Connection>>,
    max_api_keys_per_user: u64,
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

fn ts(ts: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(ts, 0).unwrap()
}

fn opt_ts(ts: Option<i64>) -> Option<DateTime<Utc>> {
    ts.map(|t| DateTime::from_timestamp(t, 0).unwrap())
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
        username: row.get(1)?,
        name: row.get(2)?,
        enabled: row.get::<_, i32>(3)? != 0,
        expires_at: opt_ts(row.get::<_, Option<i64>>(4)?),
        created_at: ts(row.get::<_, i64>(5)?),
    })
}

fn row_to_session(row: &Row) -> rusqlite::Result<Session> {
    Ok(Session {
        session_key: row.get(0)?,
        username: row.get(1)?,
        created_at: ts(row.get::<_, i64>(2)?),
        expires_at: ts(row.get::<_, i64>(3)?),
    })
}

fn row_to_user_basic(row: &Row) -> rusqlite::Result<(String, String, i64, i64)> {
    Ok((
        row.get::<_, String>(0)?,
        row.get::<_, String>(1)?,
        row.get::<_, i64>(2)?,
        row.get::<_, i64>(3)?,
    ))
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
        CREATE TABLE IF NOT EXISTS users (
            username      TEXT PRIMARY KEY,
            password_hash TEXT NOT NULL,
            created_at    INTEGER NOT NULL,
            updated_at    INTEGER NOT NULL
        );
        -- no FK on username: sessions are transient cache data, user cleanup
        -- is handled explicitly before the user row is removed
        CREATE TABLE IF NOT EXISTS sessions (
            session_key_hash TEXT PRIMARY KEY,
            username         TEXT NOT NULL,
            created_at       INTEGER NOT NULL,
            expires_at       INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_sessions_expires ON sessions(expires_at);
        CREATE TABLE IF NOT EXISTS api_keys (
            id         TEXT PRIMARY KEY,
            key_hash   TEXT NOT NULL UNIQUE,
            display    TEXT NOT NULL,
            username   TEXT NOT NULL REFERENCES users(username) ON DELETE CASCADE,
            name       TEXT NOT NULL,
            enabled    INTEGER NOT NULL DEFAULT 1,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            expires_at INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_apikeys_username ON api_keys(username);",
    )
    .map_err(to_storage)
}

// ---------------------------------------------------------------------------
// Database impl
// ---------------------------------------------------------------------------

impl Database {
    pub fn new(
        path: &str,
        max_api_keys_per_user: u64,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(parent) = Path::new(path).parent() {
            fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        create_tables(&conn)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            max_api_keys_per_user,
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

        let conn = self.conn.lock().unwrap();
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

    pub fn update_provider(&self, updates: &Provider) -> Result<Provider, DbError> {
        let conn = self.conn.lock().unwrap();
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

        provider.name = updates.name.clone();
        provider.provider_type = updates.provider_type.clone();
        provider.base_url = updates.base_url.clone();
        provider.api_key = match &updates.api_key {
            None => provider.api_key,
            Some(s) if s.is_empty() => None,
            Some(s) => Some(s.clone()),
        };
        provider.enabled = updates.enabled;
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
        let conn = self.conn.lock().unwrap();
        let rows = conn
            .execute("DELETE FROM providers WHERE id = ?1", params![id])
            .map_err(to_storage)?;
        Ok(rows > 0)
    }

    pub fn get_provider(&self, id: &str) -> Result<Option<Provider>, DbError> {
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM providers", [], |row| row.get(0))
            .map_err(to_storage)?;
        Ok(count as usize)
    }

    pub fn list_providers(&self) -> Result<Vec<Provider>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, name, type, base_url, api_key, enabled, created_at, updated_at
                 FROM providers ORDER BY name",
            )
            .map_err(to_storage)?;
        let items = stmt
            .query_map([], row_to_provider)
            .map_err(to_storage)?
            .flatten()
            .collect();
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

        let conn = self.conn.lock().unwrap();

        let (prov_exists, name_exists): (bool, bool) = conn
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

        conn.execute(
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

        Ok(model)
    }

    pub fn update_model(&self, updates: &Model) -> Result<Model, DbError> {
        let conn = self.conn.lock().unwrap();
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

        model.provider_id = updates.provider_id.clone();
        model.upstream_model = updates.upstream_model.clone();
        model.enabled = updates.enabled;
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

    pub fn delete_model(&self, name: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM models WHERE name = ?1", params![name])
            .map_err(to_storage)?;
        Ok(())
    }

    pub fn get_model(&self, name: &str) -> Result<Option<Model>, DbError> {
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM models", [], |row| row.get(0))
            .map_err(to_storage)?;
        Ok(count as usize)
    }

    pub fn list_models(&self) -> Result<Vec<Model>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, name, provider_id, upstream_model, enabled, created_at, updated_at
                 FROM models ORDER BY name",
            )
            .map_err(to_storage)?;
        let items = stmt
            .query_map([], row_to_model)
            .map_err(to_storage)?
            .flatten()
            .collect();
        Ok(items)
    }

    // --- resolve_model: hot path ---

    pub fn resolve_model(&self, model_name: &str) -> Result<Option<(Model, Provider)>, DbError> {
        let conn = self.conn.lock().unwrap();
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

    // --- User CRUD ---

    pub fn insert_user(&self, mut user: User) -> Result<User, DbError> {
        let now = Utc::now();
        user.created_at = now;
        user.updated_at = now;

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO users (username, password_hash, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                user.username,
                user.password_hash,
                user.created_at.timestamp(),
                user.updated_at.timestamp(),
            ],
        )
        .map_err(to_storage)?;

        Ok(user)
    }

    pub fn get_user(&self, username: &str) -> Result<Option<User>, DbError> {
        let conn = self.conn.lock().unwrap();

        let mut user = match conn.query_row(
            "SELECT username, password_hash, created_at, updated_at
             FROM users WHERE username = ?1",
            params![username],
            |row| {
                let (username, password_hash, created_ts, updated_ts) = row_to_user_basic(row)?;
                Ok(User {
                    username,
                    password_hash,
                    api_keys: vec![],
                    created_at: ts(created_ts),
                    updated_at: ts(updated_ts),
                })
            },
        ) {
            Ok(u) => u,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(DbError::Storage(e.to_string())),
        };

        // Fill api_keys
        let mut stmt = conn
            .prepare(
                "SELECT id, key_hash, display, name, created_at, updated_at, enabled, expires_at
                 FROM api_keys WHERE username = ?1 ORDER BY created_at",
            )
            .map_err(to_storage)?;
        let keys = stmt
            .query_map(params![username], row_to_api_key)
            .map_err(to_storage)?;

        for key in keys.flatten() {
            user.api_keys.push(key);
        }

        Ok(Some(user))
    }

    pub fn has_any_users(&self) -> Result<bool, DbError> {
        let conn = self.conn.lock().unwrap();
        let exists: bool = conn
            .query_row("SELECT EXISTS(SELECT 1 FROM users LIMIT 1)", [], |row| {
                row.get(0)
            })
            .map_err(to_storage)?;
        Ok(exists)
    }

    pub fn update_user(&self, user: &User) -> Result<User, DbError> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();

        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM users WHERE username = ?1)",
                params![user.username],
                |row| row.get(0),
            )
            .map_err(to_storage)?;
        if !exists {
            return Err(DbError::NotFound(format!(
                "User '{}' not found",
                user.username
            )));
        }

        conn.execute(
            "UPDATE users SET password_hash = ?1, updated_at = ?2 WHERE username = ?3",
            params![user.password_hash, now, user.username],
        )
        .map_err(to_storage)?;

        let mut updated = user.clone();
        updated.updated_at = Utc::now();
        Ok(updated)
    }

    // --- Session CRUD ---

    pub fn insert_session(&self, mut session: Session) -> Result<Session, DbError> {
        session.created_at = Utc::now();
        let hash = hash_key(&session.session_key);
        session.session_key = hash.clone();

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sessions (session_key_hash, username, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                hash,
                session.username,
                session.created_at.timestamp(),
                session.expires_at.timestamp(),
            ],
        )
        .map_err(to_storage)?;

        Ok(session)
    }

    pub fn get_session(&self, session_key: &str) -> Result<Option<Session>, DbError> {
        let hash = hash_key(session_key);
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT session_key_hash, username, created_at, expires_at
             FROM sessions WHERE session_key_hash = ?1",
            params![hash],
            row_to_session,
        );
        match result {
            Ok(s) => Ok(Some(s)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(DbError::Storage(e.to_string())),
        }
    }

    pub fn delete_session(&self, session_key: &str) -> Result<bool, DbError> {
        let hash = hash_key(session_key);
        let conn = self.conn.lock().unwrap();
        let rows = conn
            .execute(
                "DELETE FROM sessions WHERE session_key_hash = ?1",
                params![hash],
            )
            .map_err(to_storage)?;
        Ok(rows > 0)
    }

    pub fn cleanup_expired_sessions(&self) -> Result<usize, DbError> {
        let now = Utc::now().timestamp();
        let conn = self.conn.lock().unwrap();
        let rows = conn
            .execute("DELETE FROM sessions WHERE expires_at < ?1", params![now])
            .map_err(to_storage)?;
        Ok(rows)
    }

    // --- API Key CRUD ---

    fn generate_api_key() -> String {
        use rand::RngExt;
        const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let mut rng = rand::rng();
        let random: String = (0..32)
            .map(|_| CHARS[rng.random_range(0..CHARS.len())] as char)
            .collect();
        format!("sk-{}", random)
    }

    pub fn insert_api_key(
        &self,
        username: &str,
        name: &str,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<(ApiKey, String), DbError> {
        let raw_key = Self::generate_api_key();
        let hash = hash_key(&raw_key);
        let now = Utc::now();
        let id = Uuid::new_v4().to_string();
        let now_ts = now.timestamp();
        let expires_ts = expires_at.map(|dt| dt.timestamp());

        let conn = self.conn.lock().unwrap();

        let user_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM users WHERE username = ?1)",
                params![username],
                |row| row.get(0),
            )
            .map_err(to_storage)?;
        if !user_exists {
            return Err(DbError::NotFound(format!("User '{}' not found", username)));
        }

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM api_keys WHERE username = ?1",
                params![username],
                |row| row.get(0),
            )
            .map_err(to_storage)?;
        if count as u64 >= self.max_api_keys_per_user {
            return Err(DbError::LimitExceeded(format!(
                "Maximum of {} API keys per user",
                self.max_api_keys_per_user
            )));
        }

        conn.execute(
            "INSERT INTO api_keys (id, key_hash, display, username, name, enabled, created_at, updated_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                hash,
                mask_api_key(&raw_key),
                username,
                name,
                1i32,
                now_ts,
                now_ts,
                expires_ts,
            ],
        )
        .map_err(to_storage)?;

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

    pub fn get_user_by_api_key(&self, api_key: &str) -> Result<Option<ApiKeyInfo>, DbError> {
        let api_hash = hash_key(api_key);
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT id, username, name, enabled, expires_at, created_at
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

    pub fn delete_api_key(&self, username: &str, key_id: &str) -> Result<String, DbError> {
        let conn = self.conn.lock().unwrap();

        let hash: String = conn
            .query_row(
                "SELECT key_hash FROM api_keys WHERE id = ?1 AND username = ?2",
                params![key_id, username],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    DbError::NotFound("API key not found".to_string())
                }
                _ => DbError::Storage(e.to_string()),
            })?;

        conn.execute(
            "DELETE FROM api_keys WHERE id = ?1 AND username = ?2",
            params![key_id, username],
        )
        .map_err(to_storage)?;

        Ok(hash)
    }

    pub fn update_api_key(
        &self,
        username: &str,
        updates: &ApiKey,
    ) -> Result<(ApiKey, String), DbError> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now();

        let mut api_key = {
            let result = conn.query_row(
                "SELECT id, key_hash, display, name, created_at, updated_at, enabled, expires_at
                 FROM api_keys WHERE id = ?1 AND username = ?2",
                params![updates.id, username],
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

        api_key.name = if updates.name.is_empty() {
            api_key.name
        } else {
            updates.name.clone()
        };
        api_key.enabled = updates.enabled;
        api_key.expires_at = match updates.expires_at {
            None => api_key.expires_at,
            Some(dt) if dt.timestamp() == 0 => None,
            Some(dt) => Some(dt),
        };
        api_key.updated_at = now;

        conn.execute(
            "UPDATE api_keys SET name = ?1, enabled = ?2, expires_at = ?3, updated_at = ?4 WHERE id = ?5 AND username = ?6",
            params![
                api_key.name,
                api_key.enabled as i32,
                api_key.expires_at.map(|dt| dt.timestamp()),
                api_key.updated_at.timestamp(),
                api_key.id,
                username,
            ],
        )
        .map_err(to_storage)?;

        let hash = api_key.key.clone();
        Ok((api_key, hash))
    }

    // --- Test helpers ---

    #[cfg(test)]
    pub fn list_sessions(&self) -> Result<Vec<Session>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT session_key_hash, username, created_at, expires_at
                 FROM sessions ORDER BY created_at",
            )
            .map_err(to_storage)?;
        let items: Vec<Session> = stmt
            .query_map([], row_to_session)
            .map_err(to_storage)?
            .flatten()
            .collect();
        Ok(items)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Database, tempfile::TempDir) {
        crate::test_utils::create_test_db(10)
    }

    fn make_prov(id: &str) -> Provider {
        crate::test_utils::create_test_provider(
            id,
            ProviderType::OpenAICompat,
            "https://example.com",
        )
    }

    fn make_user() -> User {
        crate::test_utils::create_test_user()
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
        let mut updated = prov.clone();
        updated.name = "p1_renamed".to_string();
        updated.enabled = false;
        db.update_provider(&updated).unwrap();
        let got = db.get_provider("p1").unwrap().unwrap();
        assert_eq!(got.name, "p1_renamed");
        assert!(!got.enabled);
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
        let mut updated = model.clone();
        updated.provider_id = "p2".to_string();
        updated.enabled = false;
        db.update_model(&updated).unwrap();
        let got = db.get_model("m1").unwrap().unwrap();
        assert_eq!(got.provider_id, "p2");
        assert!(!got.enabled);
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

    // ── User CRUD ──

    #[test]
    fn user_create() {
        let (db, _dir) = setup();
        let user = db.insert_user(make_user()).unwrap();
        assert!(!user.username.is_empty());
    }

    #[test]
    fn user_get() {
        let (db, _dir) = setup();
        let user = db.insert_user(make_user()).unwrap();
        let got = db.get_user(&user.username).unwrap().expect("should exist");
        assert_eq!(got.username, user.username);
    }

    #[test]
    fn user_get_missing() {
        let (db, _dir) = setup();
        assert!(db.get_user("nobody").unwrap().is_none());
    }

    #[test]
    fn has_any_users_empty() {
        let (db, _dir) = setup();
        assert!(!db.has_any_users().unwrap());
    }

    #[test]
    fn has_any_users_with_users() {
        let (db, _dir) = setup();
        db.insert_user(make_user()).unwrap();
        assert!(db.has_any_users().unwrap());
    }

    #[test]
    fn user_update() {
        let (db, _dir) = setup();
        let user = make_user();
        let username = user.username.clone();
        db.insert_user(user).unwrap();
        let mut updated = db.get_user(&username).unwrap().unwrap();
        updated.password_hash = "newhash".to_string();
        db.update_user(&updated).unwrap();
        assert_eq!(
            db.get_user(&username).unwrap().unwrap().password_hash,
            "newhash"
        );
    }

    // ── Session CRUD ──

    #[test]
    fn session_insert_hashes_key() {
        let (db, _dir) = setup();
        let raw_key = "my-raw-session-key";
        let mut session = crate::test_utils::create_test_session("alice", 3600);
        session.session_key = raw_key.to_string();
        let stored = db.insert_session(session).unwrap();
        assert_ne!(stored.session_key, raw_key);
        let found = db.get_session(raw_key).unwrap().expect("should find");
        assert_eq!(found.username, "alice");
    }

    #[test]
    fn session_get() {
        let (db, _dir) = setup();
        let raw_key = "bob-session-key";
        let mut session = crate::test_utils::create_test_session("bob", 3600);
        session.session_key = raw_key.to_string();
        db.insert_session(session).unwrap();
        let found = db.get_session(raw_key).unwrap().expect("should find");
        assert_eq!(found.username, "bob");
    }

    #[test]
    fn session_get_missing() {
        let (db, _dir) = setup();
        assert!(db.get_session("no-such-key").unwrap().is_none());
    }

    #[test]
    fn session_delete() {
        let (db, _dir) = setup();
        let raw_key = "alice-session-key";
        let mut session = crate::test_utils::create_test_session("alice", 3600);
        session.session_key = raw_key.to_string();
        let s = db.insert_session(session).unwrap();
        assert!(db.delete_session(raw_key).unwrap());
        assert!(db.get_session(raw_key).unwrap().is_none());
        assert!(!db.delete_session(&s.session_key).unwrap());
    }

    #[test]
    fn cleanup_expired_sessions() {
        let (db, _dir) = setup();
        db.insert_session(crate::test_utils::create_test_session("alice", 3600))
            .unwrap();
        db.insert_session(crate::test_utils::create_expired_session("bob"))
            .unwrap();
        db.insert_session(crate::test_utils::create_expired_session("charlie"))
            .unwrap();
        let cleaned = db.cleanup_expired_sessions().unwrap();
        assert_eq!(cleaned, 2);
        let all = db.list_sessions().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].username, "alice");
    }

    #[test]
    fn delete_session_missing() {
        let (db, _dir) = setup();
        assert!(!db.delete_session("no-such-key").unwrap());
    }

    // ── API Key CRUD ──

    #[test]
    fn api_key_create_returns_raw_key() {
        let (db, _dir) = setup();
        let user = db.insert_user(make_user()).unwrap();
        let (stored, raw) = db.insert_api_key(&user.username, "test-key", None).unwrap();
        assert!(raw.starts_with("sk-"));
        assert_eq!(raw.len(), 35);
        assert_eq!(stored.name, "test-key");
    }

    #[test]
    fn api_key_lookup_by_raw_key() {
        let (db, _dir) = setup();
        let user = db.insert_user(make_user()).unwrap();
        let (_, raw) = db.insert_api_key(&user.username, "test-key", None).unwrap();
        let info = db.get_user_by_api_key(&raw).unwrap().expect("should find");
        assert_eq!(info.username, user.username);
    }

    #[test]
    fn api_key_lookup_invalid_key() {
        let (db, _dir) = setup();
        assert!(db.get_user_by_api_key("sk-invalid").unwrap().is_none());
    }

    #[test]
    fn api_key_delete() {
        let (db, _dir) = setup();
        let user = db.insert_user(make_user()).unwrap();
        let (_, raw) = db.insert_api_key(&user.username, "test-key", None).unwrap();
        let stored = db.get_user_by_api_key(&raw).unwrap().unwrap();
        db.delete_api_key(&user.username, &stored.id).unwrap();
        assert!(db.get_user_by_api_key(&raw).unwrap().is_none());
    }

    #[test]
    fn api_key_update() {
        let (db, _dir) = setup();
        let user = db.insert_user(make_user()).unwrap();
        let (stored, raw) = db.insert_api_key(&user.username, "test-key", None).unwrap();
        db.update_api_key(
            &user.username,
            &ApiKey {
                id: stored.id.clone(),
                enabled: false,
                ..Default::default()
            },
        )
        .unwrap();
        let info = db.get_user_by_api_key(&raw).unwrap().unwrap();
        let u = db.get_user(&user.username).unwrap().unwrap();
        let key = u.api_keys.iter().find(|k| k.id == info.id).unwrap();
        assert!(!key.enabled);
    }

    #[test]
    fn api_key_cross_user_isolation() {
        let (db, _dir) = setup();
        let alice = db.insert_user(make_user()).unwrap();
        let bob = db.insert_user(make_user()).unwrap();
        let (stored, _) = db
            .insert_api_key(&alice.username, "alice-key", None)
            .unwrap();
        db.delete_api_key(&bob.username, &stored.id).unwrap_err();
        let bob_user = db.get_user(&bob.username).unwrap().unwrap();
        assert_eq!(bob_user.api_keys.len(), 0);
    }

    #[test]
    fn api_key_limit_exceeded() {
        let (db, _dir) = crate::test_utils::create_test_db(2);
        let user = db.insert_user(make_user()).unwrap();
        db.insert_api_key(&user.username, "k1", None).unwrap();
        db.insert_api_key(&user.username, "k2", None).unwrap();
        let err = db.insert_api_key(&user.username, "k3", None).unwrap_err();
        assert!(matches!(err, DbError::LimitExceeded(_)));
    }

    // ── Database isolation ──

    #[test]
    fn databases_are_isolated() {
        let (db_a, _dir_a) = crate::test_utils::create_test_db(10);
        let (db_b, _dir_b) = crate::test_utils::create_test_db(10);
        db_a.insert_provider(make_prov("p1")).unwrap();
        assert!(db_b.list_providers().unwrap().is_empty());
    }
}
