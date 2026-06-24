use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct LoginResponse {
    #[allow(dead_code)]
    pub ok: bool,
    pub role: String,
}

pub fn format_timestamp(ts: f64) -> String {
    let ms = ts * 1000.0;
    let date = js_sys::Date::new(&ms.into());
    let y = date.get_full_year();
    let m = date.get_month() + 1;
    let d = date.get_date();
    let h = date.get_hours();
    let min = date.get_minutes();
    let s = date.get_seconds();
    format!("{}-{:02}-{:02} {:02}:{:02}:{:02}", y, m, d, h, min, s)
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderTypeInfo {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub provider_type: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub enabled: bool,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub provider_id: String,
    pub upstream_model: String,
    pub enabled: bool,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ApiKeyListItem {
    pub id: String,
    pub key: String,
    pub name: String,
    pub created_at: f64,
    pub enabled: bool,
    pub expires_at: Option<f64>,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateApiKeyResponse {
    pub key: String,
    pub name: String,
    #[allow(dead_code)]
    pub created_at: f64,
    #[allow(dead_code)]
    pub expires_at: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DashboardStats {
    pub provider_count: u64,
    pub model_count: u64,
    pub api_request_count: u64,
    pub token_consumption: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DailyRequests {
    pub date: String,
    pub count: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DailyTokens {
    pub date: String,
    pub tokens: u64,
}

#[derive(Debug, Clone)]
pub struct DashboardData {
    pub provider_count: u64,
    pub model_count: u64,
    pub api_request_count: u64,
    pub token_consumption: u64,
    pub daily_requests: Vec<DailyRequests>,
    pub daily_tokens: Vec<DailyTokens>,
}
