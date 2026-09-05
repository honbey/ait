//! API key persistence.
//!
//! Raw keys exist only at creation time: every later lookup goes through the
//! SHA-256 hash, so a database leak does not hand over usable credentials.

use chrono::{DateTime, Utc};
use rusqlite::{TransactionBehavior, params};
use tracing::warn;
use uuid::Uuid;

use crate::utils::mask_api_key;

use super::{
    ApiKey, ApiKeyInfo, ApiKeyUpdate, Database, DbError, hash_key, row_to_api_key,
    row_to_api_key_info, to_storage,
};

impl Database {
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

        let mut conn = self.lock_writer();
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
        let conn = self.lock_reader();
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
        let conn = self.lock_reader();
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
        let conn = self.lock_writer();

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
        let mut conn = self.lock_writer();
        let now = Utc::now();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(to_storage)?;

        let mut api_key = {
            let result = tx.query_row(
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

        tx.execute(
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
        tx.commit().map_err(to_storage)?;

        let hash = api_key.key.clone();
        Ok((api_key, hash))
    }
}
