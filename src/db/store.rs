use chrono::{DateTime, Utc};
use rocksdb::{DB as RocksDB, IteratorMode, Options, WriteBatch};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use super::models::*;

// RocksDB Column Family
const PROVIDERS_CF: &str = "providers";
const MODELS_CF: &str = "models";
const USERS_CF: &str = "users";
const SESSIONS_CF: &str = "sessions";
const API_KEYS_CF: &str = "api_keys";
const PROVIDER_MODEL_CF: &str = "provider_model";
const SESSION_EXPIRY_CF: &str = "session_expiry";

pub(crate) fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

pub struct Database {
    db: Arc<RocksDB>,
    max_api_keys_per_user: u64,
    provider_lock: Mutex<()>,
    model_lock: Mutex<()>,
    user_lock: Mutex<()>,
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
    pub fn new(
        path: &str,
        max_api_keys_per_user: u64,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(parent) = Path::new(path).parent() {
            fs::create_dir_all(parent)?;
        }

        let mut db_opts = Options::default();
        db_opts.create_if_missing(true);
        db_opts.create_missing_column_families(true);

        let cf_names = vec![
            PROVIDERS_CF,
            MODELS_CF,
            USERS_CF,
            SESSIONS_CF,
            API_KEYS_CF,
            PROVIDER_MODEL_CF,
            SESSION_EXPIRY_CF,
        ];
        let db = RocksDB::open_cf(&db_opts, path, &cf_names)?;

        Ok(Self {
            db: Arc::new(db),
            max_api_keys_per_user,
            provider_lock: Mutex::new(()),
            model_lock: Mutex::new(()),
            user_lock: Mutex::new(()),
        })
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

    fn cf_count(&self, cf_name: &str) -> Result<usize, DbError> {
        let cf = self.cf(cf_name)?;
        let mut count = 0;
        for item in self.db.iterator_cf(&cf, IteratorMode::Start) {
            match item {
                Ok(_) => count += 1,
                Err(e) => return Err(DbError::Storage(e.to_string())),
            }
        }
        Ok(count)
    }

    fn cf_put<T: serde::Serialize>(
        &self,
        cf_name: &str,
        key: impl AsRef<[u8]>,
        val: &T,
    ) -> Result<(), DbError> {
        let cf = self.cf(cf_name)?;
        let bytes = serde_json::to_vec(val).map_err(|e| DbError::Storage(e.to_string()))?;
        self.db
            .put_cf(&cf, key, bytes)
            .map_err(|e| DbError::Storage(e.to_string()))
    }

    fn cf_put_batch<T: serde::Serialize>(
        &self,
        batch: &mut WriteBatch,
        cf_name: &str,
        key: impl AsRef<[u8]>,
        val: &T,
    ) -> Result<(), DbError> {
        let cf = self.cf(cf_name)?;
        let bytes = serde_json::to_vec(val).map_err(|e| DbError::Storage(e.to_string()))?;
        batch.put_cf(cf, key, bytes);
        Ok(())
    }

    fn cf_del_batch(
        &self,
        batch: &mut WriteBatch,
        cf_name: &str,
        key: impl AsRef<[u8]>,
    ) -> Result<(), DbError> {
        let cf = self.cf(cf_name)?;
        batch.delete_cf(cf, key);
        Ok(())
    }

    fn write_batch(&self, batch: WriteBatch) -> Result<(), DbError> {
        self.db
            .write(batch)
            .map_err(|e| DbError::Storage(e.to_string()))
    }

    fn cf_prefix_keys(
        &self,
        cf_name: &str,
        prefix: impl AsRef<[u8]>,
    ) -> Result<Vec<Vec<u8>>, DbError> {
        let cf = self.cf(cf_name)?;
        let mut keys = Vec::new();
        for item in self
            .db
            .iterator_cf(
                &cf,
                IteratorMode::From(prefix.as_ref(), rocksdb::Direction::Forward),
            )
            .flatten()
        {
            if !item.0.starts_with(prefix.as_ref()) {
                break;
            }
            keys.push(item.0.to_vec());
        }
        Ok(keys)
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

    pub fn update_provider(&self, updates: &Provider) -> Result<Provider, DbError> {
        let _guard = self.provider_lock.lock().unwrap();
        let mut provider = self
            .get_provider(&updates.id)?
            .ok_or_else(|| DbError::NotFound(format!("Provider '{}' not found", &updates.id)))?;

        provider.name = updates.name.clone();
        provider.provider_type = updates.provider_type.clone();
        provider.base_url = updates.base_url.clone();
        provider.api_key = match &updates.api_key {
            None => provider.api_key,
            Some(s) if s.is_empty() => None,
            Some(s) => Some(s.clone()),
        };
        provider.enabled = updates.enabled;
        provider.updated_at = Utc::now();

        self.cf_put(PROVIDERS_CF, format!("prov:{}", &updates.id), &provider)?;
        Ok(provider)
    }

    pub fn delete_provider(&self, id: &str) -> Result<bool, DbError> {
        let _p_guard = self.provider_lock.lock().unwrap();
        let _m_guard = self.model_lock.lock().unwrap();
        if self.get_provider(id)?.is_none() {
            return Ok(false);
        }

        let mut batch = WriteBatch::default();
        self.cf_del_batch(&mut batch, PROVIDERS_CF, format!("prov:{}", id))?;

        let pm_prefix = format!("pm:{}:", id);
        let pm_keys = self.cf_prefix_keys(PROVIDER_MODEL_CF, &pm_prefix)?;
        for key in &pm_keys {
            let key_str = std::str::from_utf8(key).map_err(|e| DbError::Storage(e.to_string()))?;
            let model_name = key_str
                .strip_prefix(&pm_prefix)
                .ok_or_else(|| DbError::Storage("invalid pm key".to_string()))?;
            self.cf_del_batch(&mut batch, MODELS_CF, format!("model:{}", model_name))?;
            self.cf_del_batch(&mut batch, PROVIDER_MODEL_CF, key)?;
        }

        self.write_batch(batch)?;
        Ok(true)
    }

    pub fn get_provider(&self, id: &str) -> Result<Option<Provider>, DbError> {
        self.cf_get(PROVIDERS_CF, format!("prov:{}", id))
    }

    pub fn count_providers(&self) -> Result<usize, DbError> {
        self.cf_count(PROVIDERS_CF)
    }

    pub fn list_providers(&self) -> Result<Vec<Provider>, DbError> {
        self.cf_list(PROVIDERS_CF)
    }

    // --- Model CRUD ---

    pub fn insert_model(&self, mut model: Model) -> Result<Model, DbError> {
        if model.id.is_empty() {
            model.id = Uuid::new_v4().to_string();
        }
        let now = Utc::now();
        model.created_at = now;
        model.updated_at = now;

        // Check provider exists

        if model.provider_id.is_empty() {
            return Err(DbError::Storage("provider_id is required".to_string()));
        }
        let prov = self.get_provider(&model.provider_id)?;
        if prov.is_none() {
            return Err(DbError::NotFound(format!(
                "Provider '{}' not found",
                model.provider_id
            )));
        }

        let mut batch = WriteBatch::default();
        self.cf_put_batch(
            &mut batch,
            MODELS_CF,
            format!("model:{}", model.name),
            &model,
        )?;
        self.cf_put_batch(
            &mut batch,
            PROVIDER_MODEL_CF,
            format!("pm:{}:{}", model.provider_id, model.name),
            &String::new(),
        )?;
        self.write_batch(batch)?;
        Ok(model)
    }

    pub fn update_model(&self, updates: &Model) -> Result<Model, DbError> {
        let _guard = self.model_lock.lock().unwrap();
        let mut model = self
            .get_model(&updates.name)?
            .ok_or_else(|| DbError::NotFound(format!("Model '{}' not found", &updates.name)))?;

        let old_provider_id = model.provider_id.clone();
        model.provider_id = updates.provider_id.clone();
        model.upstream_model = updates.upstream_model.clone();
        model.enabled = updates.enabled;
        model.updated_at = Utc::now();

        let mut batch = WriteBatch::default();
        self.cf_put_batch(
            &mut batch,
            MODELS_CF,
            format!("model:{}", &updates.name),
            &model,
        )?;
        if old_provider_id != model.provider_id {
            self.cf_del_batch(
                &mut batch,
                PROVIDER_MODEL_CF,
                format!("pm:{}:{}", old_provider_id, updates.name),
            )?;
            self.cf_put_batch(
                &mut batch,
                PROVIDER_MODEL_CF,
                format!("pm:{}:{}", model.provider_id, updates.name),
                &String::new(),
            )?;
        }
        self.write_batch(batch)?;
        Ok(model)
    }

    pub fn delete_model(&self, name: &str) -> Result<(), DbError> {
        let model = match self.get_model(name)? {
            Some(m) => m,
            None => return Ok(()),
        };
        let mut batch = WriteBatch::default();
        self.cf_del_batch(&mut batch, MODELS_CF, format!("model:{}", name))?;
        self.cf_del_batch(
            &mut batch,
            PROVIDER_MODEL_CF,
            format!("pm:{}:{}", model.provider_id, name),
        )?;
        self.write_batch(batch)?;
        Ok(())
    }

    pub fn get_model(&self, name: &str) -> Result<Option<Model>, DbError> {
        self.cf_get(MODELS_CF, format!("model:{}", name))
    }

    pub fn count_models(&self) -> Result<usize, DbError> {
        self.cf_count(MODELS_CF)
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
        let now = Utc::now();
        user.created_at = now;
        user.updated_at = now;
        self.cf_put(USERS_CF, format!("user:{}", user.username), &user)?;
        Ok(user)
    }

    pub fn get_user(&self, username: &str) -> Result<Option<User>, DbError> {
        self.cf_get(USERS_CF, format!("user:{}", username))
    }

    pub fn has_any_users(&self) -> Result<bool, DbError> {
        let cf = self.cf(USERS_CF)?;
        match self.db.iterator_cf(&cf, IteratorMode::Start).next() {
            Some(Ok(_)) => Ok(true),
            Some(Err(e)) => Err(DbError::Storage(e.to_string())),
            None => Ok(false),
        }
    }

    pub fn list_users(&self) -> Result<Vec<User>, DbError> {
        self.cf_list(USERS_CF)
    }

    pub fn update_user(&self, user: &User) -> Result<User, DbError> {
        let _guard = self.user_lock.lock().unwrap();
        self.get_user(&user.username)?
            .ok_or_else(|| DbError::NotFound(format!("User '{}' not found", user.username)))?;
        let mut updated = user.clone();
        updated.updated_at = Utc::now();
        self.cf_put(USERS_CF, format!("user:{}", &updated.username), &updated)?;
        Ok(updated)
    }

    pub fn delete_user(&self, username: &str) -> Result<(), DbError> {
        let _guard = self.user_lock.lock().unwrap();
        let user = self.get_user_or_err(username)?;

        let mut batch = WriteBatch::default();

        for api_key in &user.api_keys {
            self.cf_del_batch(&mut batch, API_KEYS_CF, &api_key.key)?;
        }

        let sessions: Vec<Session> = self.cf_list(SESSIONS_CF)?;
        for session in &sessions {
            if session.username == username {
                self.cf_del_batch(
                    &mut batch,
                    SESSIONS_CF,
                    format!("sess:{}", &session.session_key),
                )?;
                self.cf_del_batch(
                    &mut batch,
                    SESSION_EXPIRY_CF,
                    format!(
                        "expire:{:020}:{}",
                        session.expires_at.timestamp(),
                        session.session_key
                    ),
                )?;
            }
        }

        self.cf_del_batch(&mut batch, USERS_CF, format!("user:{}", username))?;
        self.write_batch(batch)?;
        Ok(())
    }

    fn get_user_or_err(&self, username: &str) -> Result<User, DbError> {
        self.get_user(username)?
            .ok_or_else(|| DbError::NotFound(format!("User '{}' not found", username)))
    }

    // --- Session CRUD ---

    pub fn insert_session(&self, mut session: Session) -> Result<Session, DbError> {
        session.created_at = Utc::now();
        let hash = hash_key(&session.session_key);
        session.session_key = hash.clone();

        let mut batch = WriteBatch::default();
        self.cf_put_batch(&mut batch, SESSIONS_CF, format!("sess:{}", hash), &session)?;
        let ts = format!("expire:{:020}:{}", session.expires_at.timestamp(), hash);
        self.cf_put_batch(&mut batch, SESSION_EXPIRY_CF, ts, &String::new())?;
        self.write_batch(batch)?;
        Ok(session)
    }

    pub fn get_session(&self, session_key: &str) -> Result<Option<Session>, DbError> {
        self.cf_get(SESSIONS_CF, format!("sess:{}", hash_key(session_key)))
    }

    pub fn delete_session(&self, session_key: &str) -> Result<bool, DbError> {
        let hash = hash_key(session_key);
        let key = format!("sess:{}", hash);
        let session = match self.cf_get::<Session>(SESSIONS_CF, &key)? {
            Some(s) => s,
            None => return Ok(false),
        };
        let mut batch = WriteBatch::default();
        self.cf_del_batch(&mut batch, SESSIONS_CF, &key)?;
        let ts = format!("expire:{:020}:{}", session.expires_at.timestamp(), hash);
        self.cf_del_batch(&mut batch, SESSION_EXPIRY_CF, ts)?;
        self.write_batch(batch)?;
        Ok(true)
    }

    pub fn cleanup_expired_sessions(&self) -> Result<usize, DbError> {
        let now = Utc::now().timestamp();
        let cf = self.cf(SESSION_EXPIRY_CF)?;
        let prefix = b"expire:";
        let mut batch = WriteBatch::default();
        let mut count = 0;
        let iter = self
            .db
            .iterator_cf(&cf, IteratorMode::From(prefix, rocksdb::Direction::Forward));
        for item in iter.flatten() {
            let key = &item.0;
            if !key.starts_with(prefix) {
                break;
            }
            // key format: "expire:{:020}:{sha256_hex}" → 7 + 20 + 1 + 64 = 92 bytes
            //            "expire:"                    7-byte prefix
            //            "{:020}"                    20-byte zero-padded i64 timestamp (secs)
            //            ":"                          1-byte separator
            //            "{sha256_hex}"              64-byte SHA-256 hex digest
            let key_str = std::str::from_utf8(key).map_err(|e| DbError::Storage(e.to_string()))?;
            let ts_str = &key_str[7..27];
            let ts: i64 = ts_str
                .parse()
                .map_err(|_| DbError::Storage("invalid ts".to_string()))?;
            if ts > now {
                break;
            }
            let hash = &key_str[28..];
            self.cf_del_batch(&mut batch, SESSIONS_CF, format!("sess:{}", hash))?;
            self.cf_del_batch(&mut batch, SESSION_EXPIRY_CF, key)?;
            count += 1;
        }
        if count > 0 {
            self.write_batch(batch)?;
        }
        Ok(count)
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
        let _guard = self.user_lock.lock().unwrap();
        // Check user exists and key limit
        let mut user = self.get_user_or_err(username)?;
        if user.api_keys.len() as u64 >= self.max_api_keys_per_user {
            return Err(DbError::LimitExceeded(format!(
                "Maximum of {} API keys per user",
                self.max_api_keys_per_user
            )));
        }

        let raw_key = Self::generate_api_key();
        let hash = hash_key(&raw_key);
        let now = Utc::now();
        let id = Uuid::new_v4().to_string();

        // Stored ApiKey (key is hash, display is masked raw key)
        let stored = ApiKey {
            id: id.clone(),
            key: hash.clone(),
            display: mask_api_key(&raw_key),
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
            enabled: true,
            expires_at,
            created_at: now,
        };

        user.api_keys.push(stored.clone());
        user.updated_at = now;

        let mut batch = WriteBatch::default();
        self.cf_put_batch(&mut batch, API_KEYS_CF, &hash, &info)?;
        self.cf_put_batch(&mut batch, USERS_CF, format!("user:{}", username), &user)?;
        self.write_batch(batch)?;

        Ok((stored, raw_key))
    }

    pub fn get_user_by_api_key(&self, api_key: &str) -> Result<Option<ApiKeyInfo>, DbError> {
        self.cf_get(API_KEYS_CF, hash_key(api_key))
    }

    pub fn delete_api_key(&self, username: &str, key_id: &str) -> Result<String, DbError> {
        let _guard = self.user_lock.lock().unwrap();
        let mut user = self.get_user_or_err(username)?;

        let idx = user
            .api_keys
            .iter()
            .position(|k| k.id == key_id)
            .ok_or_else(|| DbError::NotFound("API key not found".to_string()))?;
        let hash = user.api_keys[idx].key.clone();
        user.api_keys.remove(idx);
        user.updated_at = Utc::now();

        let mut batch = WriteBatch::default();
        self.cf_del_batch(&mut batch, API_KEYS_CF, &hash)?;
        self.cf_put_batch(&mut batch, USERS_CF, format!("user:{}", username), &user)?;
        self.write_batch(batch)?;

        Ok(hash)
    }

    fn find_api_key_mut<'a>(
        api_keys: &'a mut [ApiKey],
        key_id: &str,
    ) -> Result<&'a mut ApiKey, DbError> {
        api_keys
            .iter_mut()
            .find(|k| k.id == key_id)
            .ok_or_else(|| DbError::NotFound("API key not found".to_string()))
    }

    pub fn toggle_api_key(
        &self,
        username: &str,
        key_id: &str,
        enabled: bool,
    ) -> Result<(ApiKey, String), DbError> {
        let _guard = self.user_lock.lock().unwrap();
        let mut user = self.get_user_or_err(username)?;

        let api_key = Self::find_api_key_mut(&mut user.api_keys, key_id)?;

        let hash = api_key.key.clone();
        api_key.enabled = enabled;
        api_key.updated_at = Utc::now();
        let result = api_key.clone();
        user.updated_at = Utc::now();

        let mut batch = WriteBatch::default();
        self.cf_put_batch(&mut batch, USERS_CF, format!("user:{}", username), &user)?;

        // Keep ApiKeyInfo in sync
        if let Some(mut info) = self.cf_get::<ApiKeyInfo>(API_KEYS_CF, &hash)? {
            info.enabled = enabled;
            self.cf_put_batch(&mut batch, API_KEYS_CF, &hash, &info)?;
        }

        self.write_batch(batch)?;

        Ok((result, hash))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (Database, tempfile::TempDir) {
        crate::test_utils::create_test_db(10)
    }

    // ── Provider CRUD ──

    fn make_prov(id: &str) -> Provider {
        crate::test_utils::create_test_provider(
            id,
            ProviderType::OpenAICompat,
            "https://example.com",
        )
    }

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
        let prov = make_prov("p1");
        let mut disabled = prov.clone();
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

    fn make_user() -> User {
        crate::test_utils::create_test_user()
    }

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
    fn user_list() {
        let (db, _dir) = setup();
        db.insert_user(make_user()).unwrap();
        db.insert_user(make_user()).unwrap();
        assert_eq!(db.list_users().unwrap().len(), 2);
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

    #[test]
    fn user_delete() {
        let (db, _dir) = setup();
        let user = db.insert_user(make_user()).unwrap();
        let uname = user.username.clone();
        db.delete_user(&uname).unwrap();
        assert!(db.get_user(&uname).unwrap().is_none());
    }

    // ── Session CRUD ──

    #[test]
    fn session_insert_hashes_key() {
        let (db, _dir) = setup();
        let raw_key = "my-raw-session-key";
        let mut session = crate::test_utils::create_test_session("alice", 3600);
        session.session_key = raw_key.to_string();
        let stored = db.insert_session(session).unwrap();
        // stored key should be hashed
        assert_ne!(stored.session_key, raw_key);
        // lookup with raw key should still work
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
        // stored key is hashed, deleting by hashed key (double-hash) should return false
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
        // alice's session should still exist
        let all: Vec<Session> = db.cf_list(SESSIONS_CF).unwrap();
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
        assert_eq!(raw.len(), 35); // "sk-" + 32 random chars
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
    fn api_key_toggle() {
        let (db, _dir) = setup();
        let user = db.insert_user(make_user()).unwrap();
        let (stored, raw) = db.insert_api_key(&user.username, "test-key", None).unwrap();
        db.toggle_api_key(&user.username, &stored.id, false)
            .unwrap();
        let info = db.get_user_by_api_key(&raw).unwrap().unwrap();
        // fetch user to verify the key's enabled status
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
        // bob cannot see or delete alice's key
        db.delete_api_key(&bob.username, &stored.id).unwrap_err();
        // list should show only bob's keys
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
