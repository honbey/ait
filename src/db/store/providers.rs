//! Provider persistence.
//!
//! Split out of `store.rs` so each resource keeps its own CRUD in one place;
//! the connection pool and row mappers stay on `Database`.

use chrono::Utc;
use rusqlite::{TransactionBehavior, params};
use tracing::warn;
use uuid::Uuid;

use super::{Database, DbError, Provider, ProviderUpdate, row_to_provider, to_storage};

impl Database {
    pub fn insert_provider(&self, mut provider: Provider) -> Result<Provider, DbError> {
        if provider.id.is_empty() {
            provider.id = Uuid::new_v4().to_string();
        }
        let now = Utc::now();
        provider.created_at = now;
        provider.updated_at = now;

        let conn = self.lock_writer();
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
        let mut conn = self.lock_writer();
        let now = Utc::now();
        // Read-modify-write must be atomic: two concurrent PATCHes would
        // otherwise last-write-win over each other's fields.
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(to_storage)?;

        let mut provider = {
            let result = tx.query_row(
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

        tx.execute(
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
        tx.commit().map_err(to_storage)?;

        Ok(provider)
    }

    pub fn delete_provider(&self, id: &str) -> Result<bool, DbError> {
        let conn = self.lock_writer();
        let rows = conn
            .execute("DELETE FROM providers WHERE id = ?1", params![id])
            .map_err(to_storage)?;
        Ok(rows > 0)
    }

    pub fn get_provider(&self, id: &str) -> Result<Option<Provider>, DbError> {
        let conn = self.lock_reader();
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
        let conn = self.lock_reader();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM providers", [], |row| row.get(0))
            .map_err(to_storage)?;
        Ok(count as usize)
    }

    pub fn list_providers(&self) -> Result<Vec<Provider>, DbError> {
        let conn = self.lock_reader();
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
}
