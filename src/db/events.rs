use chrono::{DateTime, Utc};

pub struct AccessEvent {
    pub timestamp: DateTime<Utc>,
    pub method: String,
    pub path: String,
    pub status: i32,
    pub latency_ms: i64,
    pub username: Option<String>,
    pub client_ip: Option<String>,
}

pub struct ProxyEvent {
    pub timestamp: DateTime<Utc>,
    pub username: Option<String>,
    pub model_name: String,
    pub provider_name: String,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
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

pub enum LogEvent {
    Access(AccessEvent),
    Proxy(ProxyEvent),
    Audit(AuditEvent),
}
