use chrono::serde::{ts_seconds, ts_seconds_option};
use chrono::{DateTime, Utc};
use rocksdb::{DB as RocksDB, IteratorMode, Options};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum UserRole {
    #[default]
    User,
    Admin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permission {
    pub provider_id: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub model_names: Vec<String>,
}

// RocksDB Column Family
const PROVIDERS_CF: &str = "providers";
const MODELS_CF: &str = "models";
const USERS_CF: &str = "users";
const SESSIONS_CF: &str = "sessions";
const API_KEYS_CF: &str = "api_keys";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub provider_type: ProviderType,
    pub base_url: String,
    pub api_key: Option<String>,
    pub enabled: bool,
    #[serde(with = "ts_seconds")]
    pub created_at: DateTime<chrono::Utc>,
    #[serde(with = "ts_seconds")]
    pub updated_at: DateTime<chrono::Utc>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderType {
    #[default]
    #[serde(rename = "openai_compat")]
    OpenAICompat,
    #[serde(rename = "deepseek")]
    DeepSeek,
    Zhipu,
    Ollama,
    Llamacpp,
}

pub fn mask_api_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 6 {
        "******".to_string()
    } else {
        let prefix: String = chars[..6].iter().collect();
        let suffix: String = chars[chars.len() - 3..].iter().collect();
        format!("{}******{}", prefix, suffix)
    }
}

impl Provider {
    pub fn masked_api_key(&self) -> Option<String> {
        self.api_key.as_ref().map(|key| mask_api_key(key))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub provider_id: String,
    pub upstream_model: String,
    pub enabled: bool,
    #[serde(with = "ts_seconds")]
    pub created_at: DateTime<chrono::Utc>,
    #[serde(with = "ts_seconds")]
    pub updated_at: DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: String,
    pub key: String,
    pub display: String,
    pub name: String,
    #[serde(with = "ts_seconds")]
    pub created_at: DateTime<chrono::Utc>,
    #[serde(with = "ts_seconds")]
    pub updated_at: DateTime<chrono::Utc>,
    pub enabled: bool,
    #[serde(
        default,
        with = "ts_seconds_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub expires_at: Option<DateTime<chrono::Utc>>,
}

/// Stored in the api_keys CF for O(1) reverse lookup by key value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyInfo {
    pub id: String,
    pub username: String,
    pub name: String,
    #[serde(with = "ts_seconds")]
    pub created_at: DateTime<chrono::Utc>,
}

impl ApiKey {
    pub fn masked(&self) -> String {
        self.display.clone()
    }

    fn mask_key(key: &str) -> String {
        mask_api_key(key)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub username: String,
    pub password_hash: String,
    pub role: UserRole,
    pub allowed: Vec<Permission>,
    pub api_keys: Vec<ApiKey>,
    #[serde(with = "ts_seconds")]
    pub created_at: DateTime<chrono::Utc>,
    #[serde(with = "ts_seconds")]
    pub updated_at: DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub session_key: String,
    pub username: String,
    #[serde(with = "ts_seconds")]
    pub created_at: DateTime<chrono::Utc>,
    #[serde(with = "ts_seconds")]
    pub expires_at: DateTime<chrono::Utc>,
}

fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

pub struct Database {
    db: Arc<RocksDB>,
}

#[derive(Debug)]
pub enum DbError {
    NotFound(String),
    LimitExceeded(String),
    Storage(String),
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbError::NotFound(msg) => write!(f, "Not found: {}", msg),
            DbError::LimitExceeded(msg) => write!(f, "Limit exceeded: {}", msg),
            DbError::Storage(msg) => write!(f, "Storage error: {}", msg),
        }
    }
}

impl std::error::Error for DbError {}

impl Database {
    pub fn new(path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(parent) = Path::new(path).parent() {
            fs::create_dir_all(parent)?;
        }

        let mut db_opts = Options::default();
        db_opts.create_if_missing(true);
        db_opts.create_missing_column_families(true);

        let cf_names = vec![PROVIDERS_CF, MODELS_CF, USERS_CF, SESSIONS_CF, API_KEYS_CF];
        let db = RocksDB::open_cf(&db_opts, path, &cf_names)?;

        Ok(Self { db: Arc::new(db) })
    }

    fn cf(&self, name: &str) -> Result<&rocksdb::ColumnFamily, DbError> {
        self.db
            .cf_handle(name)
            .ok_or_else(|| DbError::Storage(format!("CF '{}' not found", name)))
    }

    fn cf_get<T: serde::de::DeserializeOwned>(
        &self,
        cf_name: &str,
        key: impl AsRef<[u8]>,
    ) -> Result<Option<T>, DbError> {
        let cf = self.cf(cf_name)?;
        self.db
            .get_cf(&cf, key)
            .map_err(|e| DbError::Storage(e.to_string()))?
            .map(|val| serde_json::from_slice(&val).map_err(|e| DbError::Storage(e.to_string())))
            .transpose()
    }

    fn cf_list<T: serde::de::DeserializeOwned>(&self, cf_name: &str) -> Result<Vec<T>, DbError> {
        let cf = self.cf(cf_name)?;
        let mut items = Vec::new();
        for item in self.db.iterator_cf(&cf, IteratorMode::Start).flatten() {
            items.push(
                serde_json::from_slice(&item.1).map_err(|e| DbError::Storage(e.to_string()))?,
            );
        }
        Ok(items)
    }

    fn cf_put<T: serde::Serialize>(
        &self,
        cf_name: &str,
        key: impl AsRef<[u8]>,
        val: &T,
    ) -> Result<(), DbError> {
        let cf = self.cf(cf_name)?;
        let bytes = serde_json::to_string(val).map_err(|e| DbError::Storage(e.to_string()))?;
        self.db
            .put_cf(&cf, key, bytes)
            .map_err(|e| DbError::Storage(e.to_string()))
    }

    fn cf_del(&self, cf_name: &str, key: impl AsRef<[u8]>) -> Result<(), DbError> {
        let cf = self.cf(cf_name)?;
        self.db
            .delete_cf(&cf, key)
            .map_err(|e| DbError::Storage(e.to_string()))
    }

    // --- Provider CRUD ---

    pub fn insert_provider(&self, mut provider: Provider) -> Result<Provider, DbError> {
        if provider.id.is_empty() {
            provider.id = Uuid::new_v4().to_string();
        }
        let now = Utc::now();
        provider.created_at = now;
        provider.updated_at = now;

        self.cf_put(PROVIDERS_CF, format!("prov:{}", provider.id), &provider)?;
        Ok(provider)
    }

    pub fn update_provider(&self, id: &str, updates: &Provider) -> Result<Provider, DbError> {
        let mut provider = self
            .get_provider(id)?
            .ok_or_else(|| DbError::NotFound(format!("Provider '{}' not found", id)))?;

        provider.name = updates.name.clone();
        provider.provider_type = updates.provider_type.clone();
        provider.base_url = updates.base_url.clone();
        if updates.api_key.is_some() {
            provider.api_key = updates.api_key.clone();
        }
        provider.enabled = updates.enabled;
        provider.updated_at = Utc::now();

        self.cf_put(PROVIDERS_CF, format!("prov:{}", id), &provider)?;
        Ok(provider)
    }

    pub fn delete_provider(&self, id: &str) -> Result<bool, DbError> {
        if self.get_provider(id)?.is_none() {
            return Ok(false);
        }

        self.cf_del(PROVIDERS_CF, format!("prov:{}", id))?;

        // Also delete associated models
        let mut deleted_models = 0;
        for item in self.cf_list::<Model>(MODELS_CF)? {
            if item.provider_id == id {
                self.cf_del(MODELS_CF, format!("model:{}", item.name))?;
                deleted_models += 1;
            }
        }

        Ok(deleted_models >= 0)
    }

    pub fn get_provider(&self, id: &str) -> Result<Option<Provider>, DbError> {
        self.cf_get(PROVIDERS_CF, format!("prov:{}", id))
    }

    pub fn list_providers(&self) -> Result<Vec<Provider>, DbError> {
        self.cf_list(PROVIDERS_CF)
    }

    // --- Model CRUD ---

    pub fn insert_model(&self, mut model: Model) -> Result<Model, DbError> {
        if model.id.is_empty() {
            model.id = Uuid::new_v4().to_string();
        }
        model.created_at = Utc::now();
        model.updated_at = Utc::now();

        // Check provider exists
        if !model.provider_id.is_empty() {
            let prov = self.get_provider(&model.provider_id)?;
            if prov.is_none() {
                return Err(DbError::NotFound(format!(
                    "Provider '{}' not found",
                    model.provider_id
                )));
            }
        }

        self.cf_put(MODELS_CF, format!("model:{}", model.name), &model)?;
        Ok(model)
    }

    pub fn update_model(&self, name: &str, updates: &Model) -> Result<Model, DbError> {
        let mut model = self
            .get_model(name)?
            .ok_or_else(|| DbError::NotFound(format!("Model '{}' not found", name)))?;

        model.provider_id = updates.provider_id.clone();
        model.upstream_model = updates.upstream_model.clone();
        model.enabled = updates.enabled;
        model.updated_at = Utc::now();

        self.cf_put(MODELS_CF, format!("model:{}", name), &model)?;
        Ok(model)
    }

    pub fn delete_model(&self, name: &str) -> Result<(), DbError> {
        self.cf_del(MODELS_CF, format!("model:{}", name))
    }

    pub fn get_model(&self, name: &str) -> Result<Option<Model>, DbError> {
        self.cf_get(MODELS_CF, format!("model:{}", name))
    }

    pub fn list_models(&self) -> Result<Vec<Model>, DbError> {
        self.cf_list(MODELS_CF)
    }

    // --- Lookup: model name -> (Model, Provider) ---
    pub fn resolve_model(&self, model_name: &str) -> Result<Option<(Model, Provider)>, DbError> {
        let model = match self.get_model(model_name)? {
            Some(m) if m.enabled => m,
            Some(_) => return Ok(None), // model disabled
            None => return Ok(None),
        };

        let provider = match self.get_provider(&model.provider_id)? {
            Some(p) if p.enabled => p,
            Some(_) => return Ok(None), // provider disabled
            None => return Ok(None),
        };

        Ok(Some((model, provider)))
    }

    // --- User CRUD ---

    pub fn insert_user(&self, mut user: User) -> Result<User, DbError> {
        user.created_at = Utc::now();
        user.updated_at = Utc::now();
        self.cf_put(USERS_CF, format!("user:{}", user.username), &user)?;
        Ok(user)
    }

    pub fn get_user(&self, username: &str) -> Result<Option<User>, DbError> {
        self.cf_get(USERS_CF, format!("user:{}", username))
    }

    pub fn list_users(&self) -> Result<Vec<User>, DbError> {
        self.cf_list(USERS_CF)
    }

    pub fn update_user(&self, username: &str, mut user: User) -> Result<User, DbError> {
        let existing = self.get_user(username)?;
        match existing {
            Some(existing_user) => {
                user.created_at = existing_user.created_at;
                user.updated_at = Utc::now();
                self.cf_put(USERS_CF, format!("user:{}", username), &user)?;
                Ok(user)
            }
            None => Err(DbError::NotFound(format!("User '{}' not found", username))),
        }
    }

    pub fn delete_user(&self, username: &str) -> Result<bool, DbError> {
        self.cf_del(USERS_CF, format!("user:{}", username))?;
        Ok(true)
    }

    // --- Session CRUD ---

    pub fn insert_session(&self, mut session: Session) -> Result<Session, DbError> {
        session.created_at = Utc::now();
        let hash = hash_key(&session.session_key);
        session.session_key = hash.clone();
        self.cf_put(SESSIONS_CF, format!("sess:{}", hash), &session)?;
        Ok(session)
    }

    pub fn get_session(&self, session_key: &str) -> Result<Option<Session>, DbError> {
        self.cf_get(SESSIONS_CF, format!("sess:{}", hash_key(session_key)))
    }

    pub fn delete_session(&self, session_key: &str) -> Result<bool, DbError> {
        self.cf_del(SESSIONS_CF, format!("sess:{}", hash_key(session_key)))?;
        Ok(true)
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
        // Check user exists and key limit
        let mut user = self
            .get_user(username)?
            .ok_or_else(|| DbError::NotFound(format!("User '{}' not found", username)))?;
        if user.api_keys.len() >= 10 {
            return Err(DbError::LimitExceeded(
                "Maximum of 10 API keys per user".to_string(),
            ));
        }

        let raw_key = Self::generate_api_key();
        let hash = hash_key(&raw_key);
        let now = Utc::now();
        let id = Uuid::new_v4().to_string();

        // Stored ApiKey (key is hash, display is masked raw key)
        let stored = ApiKey {
            id: id.clone(),
            key: hash.clone(),
            display: ApiKey::mask_key(&raw_key),
            name: name.to_string(),
            created_at: now,
            updated_at: now,
            enabled: true,
            expires_at,
        };

        // Store in api_keys CF for reverse lookup
        let info = ApiKeyInfo {
            id: id.clone(),
            username: username.to_string(),
            name: name.to_string(),
            created_at: now,
        };
        self.cf_put(API_KEYS_CF, &hash, &info)?;

        user.api_keys.push(stored.clone());
        self.update_user(username, user)?;

        Ok((stored, raw_key))
    }

    pub fn get_user_by_api_key(&self, api_key: &str) -> Result<Option<ApiKeyInfo>, DbError> {
        self.cf_get(API_KEYS_CF, hash_key(api_key))
    }

    pub fn delete_api_key(&self, username: &str, key_id: &str) -> Result<bool, DbError> {
        let mut user = self
            .get_user(username)?
            .ok_or_else(|| DbError::NotFound(format!("User '{}' not found", username)))?;

        let hash = user
            .api_keys
            .iter()
            .find(|k| k.id == key_id)
            .map(|k| k.key.clone())
            .ok_or_else(|| DbError::NotFound("API key not found".to_string()))?;

        self.cf_del(API_KEYS_CF, &hash)?;

        user.api_keys.retain(|k| k.id != key_id);
        self.update_user(username, user)?;

        Ok(true)
    }

    pub fn toggle_api_key(
        &self,
        username: &str,
        key_id: &str,
        enabled: bool,
    ) -> Result<ApiKey, DbError> {
        let mut user = self
            .get_user(username)?
            .ok_or_else(|| DbError::NotFound(format!("User '{}' not found", username)))?;

        let api_key = user
            .api_keys
            .iter_mut()
            .find(|k| k.id == key_id)
            .ok_or_else(|| DbError::NotFound("API key not found".to_string()))?;

        api_key.enabled = enabled;
        api_key.updated_at = Utc::now();
        let result = api_key.clone();
        self.update_user(username, user)?;

        Ok(result)
    }
}
