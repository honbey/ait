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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
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
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub provider_type: ProviderType,
    pub base_url: String,
    pub api_key: Option<String>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(with = "ts_seconds", default)]
    pub created_at: DateTime<chrono::Utc>,
    #[serde(with = "ts_seconds", default)]
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
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub provider_id: String,
    pub upstream_model: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(with = "ts_seconds", default)]
    pub created_at: DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    #[serde(default)]
    pub id: String,
    pub key: String,
    pub display: String,
    pub name: String,
    #[serde(with = "ts_seconds", default)]
    pub created_at: DateTime<chrono::Utc>,
    #[serde(default)]
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
    #[serde(with = "ts_seconds", default)]
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
    #[serde(default)]
    pub role: UserRole,
    #[serde(default)]
    pub allowed: Vec<Permission>,
    #[serde(default)]
    pub api_keys: Vec<ApiKey>,
    #[serde(with = "ts_seconds", default)]
    pub created_at: DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub session_key: String,
    pub username: String,
    #[serde(with = "ts_seconds", default)]
    pub created_at: DateTime<chrono::Utc>,
    #[serde(with = "ts_seconds")]
    pub expires_at: DateTime<chrono::Utc>,
}

fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub struct Database {
    db: Arc<RocksDB>,
}

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

    fn cf(&self, name: &str) -> Result<&rocksdb::ColumnFamily, String> {
        self.db
            .cf_handle(name)
            .ok_or_else(|| format!("CF '{}' not found", name))
    }

    // --- Provider CRUD ---

    pub fn insert_provider(&self, mut provider: Provider) -> Result<Provider, String> {
        if provider.id.is_empty() {
            provider.id = Uuid::new_v4().to_string();
        }
        let now = Utc::now();
        provider.created_at = now;
        provider.updated_at = now;

        let key = format!("prov:{}", provider.id);
        let val = serde_json::to_string(&provider).map_err(|e| e.to_string())?;
        let cf = self.cf(PROVIDERS_CF)?;
        self.db.put_cf(&cf, &key, &val).map_err(|e| e.to_string())?;
        Ok(provider)
    }

    pub fn update_provider(
        &self,
        id: &str,
        updates: &Provider,
    ) -> Result<Option<Provider>, String> {
        let existing = self.get_provider(id)?;
        let mut provider = existing.ok_or("Provider not found")?;

        provider.name = updates.name.clone();
        provider.provider_type = updates.provider_type.clone();
        provider.base_url = updates.base_url.clone();
        if updates.api_key.is_some() {
            provider.api_key = updates.api_key.clone();
        }
        provider.enabled = updates.enabled;
        provider.updated_at = Utc::now();

        let key = format!("prov:{}", provider.id);
        let val = serde_json::to_string(&provider).map_err(|e| e.to_string())?;
        let cf = self.cf(PROVIDERS_CF)?;
        self.db.put_cf(&cf, &key, &val).map_err(|e| e.to_string())?;

        Ok(Some(provider))
    }

    pub fn delete_provider(&self, id: &str) -> Result<bool, String> {
        let key = format!("prov:{}", id);
        let cf = self.cf(PROVIDERS_CF)?;
        self.db.delete_cf(&cf, &key).map_err(|e| e.to_string())?;

        // Also delete associated models
        let mut deleted_models = 0;
        let cf_models = self.cf(MODELS_CF)?;
        for item in self
            .db
            .iterator_cf(&cf_models, IteratorMode::Start)
            .flatten()
        {
            let model: Model = serde_json::from_slice(&item.1).map_err(|e| e.to_string())?;
            if model.provider_id == id {
                self.db
                    .delete_cf(&cf_models, item.0)
                    .map_err(|e| e.to_string())?;
                deleted_models += 1;
            }
        }

        Ok(deleted_models >= 0)
    }

    pub fn get_provider(&self, id: &str) -> Result<Option<Provider>, String> {
        let key = format!("prov:{}", id);
        let cf = self.cf(PROVIDERS_CF)?;
        self.db
            .get_cf(&cf, &key)
            .map_err(|e| e.to_string())?
            .map(|val| serde_json::from_slice(&val).map_err(|e| e.to_string()))
            .transpose()
    }

    pub fn list_providers(&self) -> Result<Vec<Provider>, String> {
        let cf = self.cf(PROVIDERS_CF)?;
        let mut providers = Vec::new();
        for item in self.db.iterator_cf(&cf, IteratorMode::Start).flatten() {
            let provider: Provider = serde_json::from_slice(&item.1).map_err(|e| e.to_string())?;
            providers.push(provider);
        }
        Ok(providers)
    }

    // --- Model CRUD ---

    pub fn insert_model(&self, mut model: Model) -> Result<Model, String> {
        if model.id.is_empty() {
            model.id = Uuid::new_v4().to_string();
        }
        model.created_at = Utc::now();

        // Check provider exists
        if !model.provider_id.is_empty() {
            let prov = self.get_provider(&model.provider_id)?;
            if prov.is_none() {
                return Err(format!("Provider '{}' not found", model.provider_id));
            }
        }

        let key = format!("model:{}", model.name);
        let val = serde_json::to_string(&model).map_err(|e| e.to_string())?;
        let cf = self.cf(MODELS_CF)?;
        self.db.put_cf(&cf, &key, &val).map_err(|e| e.to_string())?;
        Ok(model)
    }

    pub fn update_model(&self, name: &str, updates: &Model) -> Result<Model, String> {
        let mut model = self
            .get_model(name)?
            .ok_or_else(|| format!("Model '{}' not found", name))?;

        model.provider_id = updates.provider_id.clone();
        model.upstream_model = updates.upstream_model.clone();
        model.enabled = updates.enabled;

        let key = format!("model:{}", model.name);
        let val = serde_json::to_string(&model).map_err(|e| e.to_string())?;
        let cf = self.cf(MODELS_CF)?;
        self.db.put_cf(&cf, &key, &val).map_err(|e| e.to_string())?;
        Ok(model)
    }

    pub fn delete_model(&self, name: &str) -> Result<(), String> {
        let key = format!("model:{}", name);
        let cf = self.cf(MODELS_CF)?;
        self.db.delete_cf(&cf, &key).map_err(|e| e.to_string())
    }

    pub fn get_model(&self, name: &str) -> Result<Option<Model>, String> {
        let key = format!("model:{}", name);
        let cf = self.cf(MODELS_CF)?;
        self.db
            .get_cf(&cf, &key)
            .map_err(|e| e.to_string())?
            .map(|val| serde_json::from_slice(&val).map_err(|e| e.to_string()))
            .transpose()
    }

    pub fn list_models(&self) -> Result<Vec<Model>, String> {
        let cf = self.cf(MODELS_CF)?;
        let mut models = Vec::new();
        for item in self.db.iterator_cf(&cf, IteratorMode::Start).flatten() {
            let model: Model = serde_json::from_slice(&item.1).map_err(|e| e.to_string())?;
            models.push(model);
        }
        Ok(models)
    }

    // --- Lookup: model name -> (Model, Provider) ---
    pub fn resolve_model(&self, model_name: &str) -> Result<Option<(Model, Provider)>, String> {
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

    pub fn insert_user(&self, mut user: User) -> Result<User, String> {
        user.created_at = Utc::now();
        let key = format!("user:{}", user.username);
        let val = serde_json::to_string(&user).map_err(|e| e.to_string())?;
        let cf = self.cf(USERS_CF)?;
        self.db.put_cf(&cf, &key, &val).map_err(|e| e.to_string())?;
        Ok(user)
    }

    pub fn get_user(&self, username: &str) -> Result<Option<User>, String> {
        let key = format!("user:{}", username);
        let cf = self.cf(USERS_CF)?;
        self.db
            .get_cf(&cf, &key)
            .map_err(|e| e.to_string())?
            .map(|val| serde_json::from_slice(&val).map_err(|e| e.to_string()))
            .transpose()
    }

    pub fn list_users(&self) -> Result<Vec<User>, String> {
        let cf = self.cf(USERS_CF)?;
        let mut users = Vec::new();
        for item in self.db.iterator_cf(&cf, IteratorMode::Start).flatten() {
            let user: User = serde_json::from_slice(&item.1).map_err(|e| e.to_string())?;
            users.push(user);
        }
        Ok(users)
    }

    pub fn update_user(&self, username: &str, mut user: User) -> Result<User, String> {
        let key = format!("user:{}", username);
        let cf = self.cf(USERS_CF)?;

        let existing = self.get_user(username)?;
        match existing {
            Some(_) => {
                user.created_at = Utc::now();
                let val = serde_json::to_string(&user).map_err(|e| e.to_string())?;
                self.db.put_cf(&cf, &key, &val).map_err(|e| e.to_string())?;
                Ok(user)
            }
            None => Err(format!("User '{}' not found", username)),
        }
    }

    pub fn delete_user(&self, username: &str) -> Result<bool, String> {
        let key = format!("user:{}", username);
        let cf = self.cf(USERS_CF)?;
        self.db.delete_cf(&cf, &key).map_err(|e| e.to_string())?;
        Ok(true)
    }

    // --- Session CRUD ---

    pub fn insert_session(&self, mut session: Session) -> Result<Session, String> {
        session.created_at = Utc::now();
        let hash = hash_key(&session.session_key);
        session.session_key = hash.clone();
        let key = format!("sess:{}", hash);
        let val = serde_json::to_string(&session).map_err(|e| e.to_string())?;
        let cf = self.cf(SESSIONS_CF)?;
        self.db.put_cf(&cf, &key, &val).map_err(|e| e.to_string())?;
        Ok(session)
    }

    pub fn get_session(&self, session_key: &str) -> Result<Option<Session>, String> {
        let hash = hash_key(session_key);
        let key = format!("sess:{}", hash);
        let cf = self.cf(SESSIONS_CF)?;
        self.db
            .get_cf(&cf, &key)
            .map_err(|e| e.to_string())?
            .map(|val| serde_json::from_slice(&val).map_err(|e| e.to_string()))
            .transpose()
    }

    pub fn delete_session(&self, session_key: &str) -> Result<bool, String> {
        let hash = hash_key(session_key);
        let key = format!("sess:{}", hash);
        let cf = self.cf(SESSIONS_CF)?;
        self.db.delete_cf(&cf, &key).map_err(|e| e.to_string())?;
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
    ) -> Result<(ApiKey, String), String> {
        // Check user exists and key limit
        let mut user = self
            .get_user(username)?
            .ok_or_else(|| format!("User '{}' not found", username))?;
        if user.api_keys.len() >= 10 {
            return Err("Maximum of 10 API keys per user".to_string());
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
        let cf = self.cf(API_KEYS_CF)?;
        self.db
            .put_cf(
                &cf,
                &hash,
                serde_json::to_string(&info).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;

        user.api_keys.push(stored.clone());
        self.update_user(username, user)?;

        Ok((stored, raw_key))
    }

    pub fn get_user_by_api_key(&self, api_key: &str) -> Result<Option<ApiKeyInfo>, String> {
        let hash = hash_key(api_key);
        let cf = self.cf(API_KEYS_CF)?;
        self.db
            .get_cf(&cf, &hash)
            .map_err(|e| e.to_string())?
            .map(|val| serde_json::from_slice(&val).map_err(|e| e.to_string()))
            .transpose()
    }

    pub fn delete_api_key(&self, username: &str, key_id: &str) -> Result<bool, String> {
        let mut user = self
            .get_user(username)?
            .ok_or_else(|| format!("User '{}' not found", username))?;

        let hash = user
            .api_keys
            .iter()
            .find(|k| k.id == key_id)
            .map(|k| k.key.clone())
            .ok_or_else(|| "API key not found".to_string())?;

        let cf = self.cf(API_KEYS_CF)?;
        self.db.delete_cf(&cf, &hash).map_err(|e| e.to_string())?;

        user.api_keys.retain(|k| k.id != key_id);
        self.update_user(username, user)?;

        Ok(true)
    }

    pub fn toggle_api_key(
        &self,
        username: &str,
        key_id: &str,
        enabled: bool,
    ) -> Result<ApiKey, String> {
        let mut user = self
            .get_user(username)?
            .ok_or_else(|| format!("User '{}' not found", username))?;

        let api_key = user
            .api_keys
            .iter_mut()
            .find(|k| k.id == key_id)
            .ok_or_else(|| "API key not found".to_string())?;

        api_key.enabled = enabled;
        let result = api_key.clone();
        self.update_user(username, user)?;

        Ok(result)
    }
}
