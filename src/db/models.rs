use chrono::serde::{ts_seconds, ts_seconds_option};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub fn mask_api_key(key: &str) -> String {
    if key.len() <= 9 {
        "******".to_string()
    } else {
        let prefix = &key[..6];
        let suffix = &key[key.len() - 3..];
        format!("{}******{}", prefix, suffix)
    }
}

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

#[derive(
    Default,
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    strum::AsRefStr,
    strum::EnumMessage,
    strum::EnumIter,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ProviderType {
    #[default]
    #[serde(rename = "openai_compat")]
    #[strum(serialize = "openai_compat", message = "OpenAI Compatible")]
    OpenAICompat,
    #[serde(rename = "deepseek")]
    #[strum(serialize = "deepseek", message = "DeepSeek")]
    DeepSeek,
    #[strum(message = "Zhipu")]
    Zhipu,
    #[strum(message = "Ollama")]
    Ollama,
    #[strum(message = "llama.cpp")]
    Llamacpp,
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

impl ApiKey {
    pub fn masked(&self) -> String {
        self.display.clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyInfo {
    pub id: String,
    pub username: String,
    pub name: String,
    pub enabled: bool,
    #[serde(
        default,
        with = "ts_seconds_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub expires_at: Option<DateTime<chrono::Utc>>,
    #[serde(with = "ts_seconds")]
    pub created_at: DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub username: String,
    pub password_hash: String,
    pub api_keys: Vec<ApiKey>,
    #[serde(with = "ts_seconds")]
    pub created_at: DateTime<chrono::Utc>,
    #[serde(with = "ts_seconds")]
    pub updated_at: DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionUser {
    pub username: String,
    pub api_key_name: Option<String>,
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

impl Session {
    pub fn is_expired(&self) -> bool {
        self.expires_at <= Utc::now()
    }
}

pub struct AccessEvent {
    pub timestamp: DateTime<Utc>,
    pub method: String,
    pub path: String,
    pub status: i32,
    pub latency_ms: i64,
    pub username: Option<String>,
    pub client_ip: Option<String>,
}

#[derive(Clone)]
pub struct ProxyEvent {
    pub timestamp: DateTime<Utc>,
    pub username: Option<String>,
    pub api_key_name: Option<String>,
    pub model_name: String,
    pub provider_name: String,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cached_tokens: Option<i64>,
    pub latency_ms: i64,
    pub status: String,
}

pub struct AuditEvent {
    pub timestamp: DateTime<Utc>,
    pub username: String,
    pub action: String,
    pub resource: String,
    pub resource_id: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketEntry {
    pub timestamp: f64,
    pub count: u64,
}

pub enum LogEvent {
    Access(AccessEvent),
    Proxy(ProxyEvent),
    Audit(AuditEvent),
    Shutdown,
}
