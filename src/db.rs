use chrono::serde::ts_seconds;
use chrono::{DateTime, Utc};
use rocksdb::{DB as RocksDB, IteratorMode, Options};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

// RocksDB Column Family
const PROVIDERS_CF: &str = "providers";
const MODELS_CF: &str = "models";

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
    OpenAICompat,
    DeepSeek,
    Zhipu,
    Ollama,
    LlamaCpp,
}

impl Provider {
    /// Return a masked version of the API key for safe display.
    /// None -> null, short keys -> "******", long keys -> first4 + "******" + last4
    pub fn masked_api_key(&self) -> Option<String> {
        self.api_key.as_ref().map(|key| {
            let chars: Vec<char> = key.chars().collect();
            if chars.len() <= 6 {
                "******".to_string()
            } else {
                let prefix: String = chars[..4].iter().collect();
                let suffix: String = chars[chars.len() - 4..].iter().collect();
                format!("{}******{}", prefix, suffix)
            }
        })
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

        let cf_names = vec![PROVIDERS_CF, MODELS_CF];
        let db = RocksDB::open_cf(&db_opts, path, &cf_names)?;

        Ok(Self { db: Arc::new(db) })
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
        let cf = self
            .db
            .cf_handle(PROVIDERS_CF)
            .ok_or("providers CF not found")?;
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
        let cf = self
            .db
            .cf_handle(PROVIDERS_CF)
            .ok_or("providers CF not found")?;
        self.db.put_cf(&cf, &key, &val).map_err(|e| e.to_string())?;

        Ok(Some(provider))
    }

    pub fn delete_provider(&self, id: &str) -> Result<bool, String> {
        let key = format!("prov:{}", id);
        let cf = self
            .db
            .cf_handle(PROVIDERS_CF)
            .ok_or("providers CF not found")?;
        self.db.delete_cf(&cf, &key).map_err(|e| e.to_string())?;

        // Also delete associated models
        let mut deleted_models = 0;
        let cf_models = self.db.cf_handle(MODELS_CF).ok_or("models CF not found")?;
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
        let cf = self
            .db
            .cf_handle(PROVIDERS_CF)
            .ok_or("providers CF not found")?;
        self.db
            .get_cf(&cf, &key)
            .map_err(|e| e.to_string())?
            .map(|val| serde_json::from_slice(&val).map_err(|e| e.to_string()))
            .transpose()
    }

    pub fn list_providers(&self) -> Result<Vec<Provider>, String> {
        let cf = self
            .db
            .cf_handle(PROVIDERS_CF)
            .ok_or("providers CF not found")?;
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
        let cf = self.db.cf_handle(MODELS_CF).ok_or("models CF not found")?;
        self.db.put_cf(&cf, &key, &val).map_err(|e| e.to_string())?;
        Ok(model)
    }

    pub fn delete_model(&self, name: &str) -> Result<(), String> {
        let key = format!("model:{}", name);
        let cf = self.db.cf_handle(MODELS_CF).ok_or("models CF not found")?;
        self.db.delete_cf(&cf, &key).map_err(|e| e.to_string())
    }

    pub fn get_model(&self, name: &str) -> Result<Option<Model>, String> {
        let key = format!("model:{}", name);
        let cf = self.db.cf_handle(MODELS_CF).ok_or("models CF not found")?;
        self.db
            .get_cf(&cf, &key)
            .map_err(|e| e.to_string())?
            .map(|val| serde_json::from_slice(&val).map_err(|e| e.to_string()))
            .transpose()
    }

    pub fn list_models(&self) -> Result<Vec<Model>, String> {
        let cf = self.db.cf_handle(MODELS_CF).ok_or("models CF not found")?;
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
}

// Database is safe to share across threads (Arc internally)
unsafe impl Send for Database {}
unsafe impl Sync for Database {}
