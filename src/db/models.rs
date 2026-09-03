use chrono::serde::{ts_seconds, ts_seconds_option};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub use crate::utils::mask_api_key;

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
    strum::EnumString,
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

impl ProviderType {
    pub fn to_db(&self) -> &str {
        self.as_ref()
    }

    pub fn from_db(s: &str) -> Self {
        s.parse().unwrap_or_default()
    }

    pub fn supports_endpoint(&self, path: &str) -> bool {
        match self {
            Self::DeepSeek => matches!(path, "/chat/completions" | "/responses"),
            Self::Zhipu => matches!(path, "/chat/completions" | "/embeddings"),
            _ => matches!(
                path,
                "/chat/completions" | "/completions" | "/embeddings" | "/responses"
            ),
        }
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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

/// Partial update patch for a provider: `None` fields are left unchanged.
#[derive(Debug, Clone, Default)]
pub struct ProviderUpdate {
    pub id: String,
    pub name: Option<String>,
    pub provider_type: Option<ProviderType>,
    pub base_url: Option<String>,
    /// `None` keeps the stored value, `Some("")` clears it.
    pub api_key: Option<String>,
    pub enabled: Option<bool>,
}

/// Partial update patch for a model: `None` fields are left unchanged.
#[derive(Debug, Clone, Default)]
pub struct ModelUpdate {
    pub name: String,
    pub provider_id: Option<String>,
    pub upstream_model: Option<String>,
    pub enabled: Option<bool>,
}

/// Partial update patch for an API key: `None` fields are left unchanged.
#[derive(Debug, Clone, Default)]
pub struct ApiKeyUpdate {
    pub id: String,
    pub name: Option<String>,
    pub enabled: Option<bool>,
    /// `None` keeps the stored value, `Some(dt)` with `dt.timestamp() == 0` clears it.
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyInfo {
    pub id: String,
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

/// Identity of the API key used for a proxy request. Injected by the proxy
/// auth middleware so handlers can record which key served a request.
#[derive(Debug, Clone)]
pub struct ApiKeyContext {
    pub name: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct AccessEvent {
    pub timestamp: DateTime<Utc>,
    pub request_id: String,
    pub method: String,
    pub path: String,
    pub status: i32,
    pub latency_ms: i64,
    pub client_ip: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct ProxyEvent {
    pub timestamp: DateTime<Utc>,
    pub request_id: String,
    pub api_key_name: Option<String>,
    pub model_name: String,
    pub provider_name: String,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cached_tokens: Option<i64>,
    pub latency_ms: i64,
    pub status: String,
    pub endpoint: String,
    pub is_streaming: bool,
    pub time_to_first_token_ms: Option<i64>,
    pub upstream_model: String,
    pub provider_type: String,
    pub response_body_size: Option<i64>,
    pub error_message: Option<String>,
    pub client_ip: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct AuditEvent {
    pub timestamp: DateTime<Utc>,
    pub request_id: String,
    pub action: String,
    pub resource: String,
    pub resource_id: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketEntry {
    pub timestamp: i64,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDistEntry {
    pub model: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenDistEntry {
    pub category: String,
    pub count: u64,
}

/// All analytics aggregates the console overview needs, produced by a single
/// analytics-worker round trip instead of one request per metric.
#[derive(Debug, Clone, Serialize, Default)]
pub struct OverviewMetrics {
    pub total_requests: u64,
    pub total_tokens: u64,
    pub request_buckets: Vec<BucketEntry>,
    pub token_buckets: Vec<BucketEntry>,
    pub model_dist: Vec<ModelDistEntry>,
    pub token_dist: Vec<TokenDistEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ProxyLogEntryResponse {
    pub timestamp: i64,
    pub api_key_name: Option<String>,
    pub model_name: String,
    pub provider_name: String,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cached_tokens: Option<i64>,
    pub latency_ms: i64,
    pub status: String,
    pub endpoint: String,
    pub is_streaming: bool,
    pub time_to_first_token_ms: Option<i64>,
    pub upstream_model: String,
    pub provider_type: String,
    pub response_body_size: Option<i64>,
    pub error_message: Option<String>,
    pub client_ip: Option<String>,
}

#[derive(Serialize)]
pub struct PaginatedResponse<T: Serialize> {
    pub items: Vec<T>,
    pub total: u64,
    pub page: u64,
    pub per_page: u64,
}

#[derive(Default)]
pub struct ProxyLogQueryParams {
    pub page: u64,
    pub per_page: u64,
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
    pub model_name: Option<String>,
    pub provider_name: Option<String>,
    pub status: Option<String>,
    pub api_key_name: Option<String>,
    pub endpoint: Option<String>,
    pub is_streaming: Option<bool>,
    pub upstream_model: Option<String>,
    pub provider_type: Option<String>,
    pub client_ip: Option<String>,
}

pub struct ProxyLogQueryResult {
    pub items: Vec<ProxyLogEntryResponse>,
    pub total: u64,
}

#[derive(Debug, Clone)]
pub struct RequestId(pub String);

#[derive(Clone)]
pub enum LogEvent {
    Access(AccessEvent),
    Proxy(Box<ProxyEvent>),
    Audit(AuditEvent),
    Shutdown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deepseek_supports_chat_and_responses_endpoints() {
        let t = ProviderType::DeepSeek;
        assert!(t.supports_endpoint("/chat/completions"));
        assert!(t.supports_endpoint("/responses"));
        assert!(!t.supports_endpoint("/completions"));
        assert!(!t.supports_endpoint("/embeddings"));
    }

    #[test]
    fn openai_compat_supports_all_endpoints() {
        let t = ProviderType::OpenAICompat;
        for path in [
            "/chat/completions",
            "/completions",
            "/embeddings",
            "/responses",
        ] {
            assert!(t.supports_endpoint(path), "{path} should be allowed");
        }
    }
}
