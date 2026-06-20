use serde::Deserialize;

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
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ApiKeyListItem {
    pub id: String,
    pub key: String,
    pub name: String,
    pub created_at: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateApiKeyResponse {
    pub key: String,
    pub name: String,
    #[allow(dead_code)]
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct DashboardData {
    pub providers: Vec<Provider>,
    pub models: Vec<Model>,
    pub api_request_count: u64,
    pub token_consumption: u64,
}
