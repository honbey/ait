//! Model persistence, including the join query the proxy path resolves
//! through (`resolve_model`).

use chrono::Utc;
use rusqlite::{TransactionBehavior, params};
use tracing::warn;
use uuid::Uuid;

use super::{
    Database, DbError, Model, ModelUpdate, Provider, ProviderType, row_to_model, to_storage, ts,
};

impl Database {
    pub fn insert_model(&self, mut model: Model) -> Result<Model, DbError> {
        if model.id.is_empty() {
            model.id = Uuid::new_v4().to_string();
        }
        let now = Utc::now();
        model.created_at = now;
        model.updated_at = now;

        let mut conn = self.lock_writer();
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
        let mut conn = self.lock_writer();
        let now = Utc::now();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(to_storage)?;

        let mut model = {
            let result = tx.query_row(
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

        tx.execute(
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
        tx.commit().map_err(to_storage)?;

        Ok(model)
    }

    pub fn delete_model(&self, name: &str) -> Result<bool, DbError> {
        let conn = self.lock_writer();
        let rows = conn
            .execute("DELETE FROM models WHERE name = ?1", params![name])
            .map_err(to_storage)?;
        Ok(rows > 0)
    }

    pub fn get_model(&self, name: &str) -> Result<Option<Model>, DbError> {
        let conn = self.lock_reader();
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
        let conn = self.lock_reader();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM models", [], |row| row.get(0))
            .map_err(to_storage)?;
        Ok(count as usize)
    }

    pub fn list_models(&self) -> Result<Vec<Model>, DbError> {
        let conn = self.lock_reader();
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
        let conn = self.lock_reader();
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
}
