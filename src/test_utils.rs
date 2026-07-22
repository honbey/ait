use crate::db::{Database, Model, Provider, ProviderType, User};
use chrono::Utc;
use tempfile::TempDir;

pub fn create_test_db(max_api_keys: u64) -> (Database, TempDir) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.db");
    let db = Database::new(path.to_str().unwrap(), max_api_keys).unwrap();
    (db, dir)
}

pub fn create_test_user() -> User {
    let password_hash = bcrypt::hash("password123", 4).unwrap();
    User {
        username: uuid::Uuid::new_v4().to_string(),
        password_hash,
        api_keys: vec![],
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

pub fn create_test_provider(id: &str, provider_type: ProviderType, base_url: &str) -> Provider {
    Provider {
        id: id.to_string(),
        name: id.to_string(),
        provider_type,
        base_url: base_url.to_string(),
        api_key: None,
        enabled: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

pub fn create_test_model(name: &str, provider_id: &str) -> Model {
    Model {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.to_string(),
        provider_id: provider_id.to_string(),
        upstream_model: name.to_string(),
        enabled: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}
