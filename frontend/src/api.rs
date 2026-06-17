use serde::de::DeserializeOwned;

use gloo_net::Error as NetError;
use gloo_net::http::{Headers, Request};

use crate::models::{DashboardData, Model, Provider};

// Admin token for testing (from config/ait.toml)
const ADMIN_TOKEN: &str = "";

/// Get base URL from window.location.origin at runtime
fn get_base_url() -> String {
    sycamore::web::window()
        .location()
        .origin()
        .unwrap_or_default()
}

// --- Mock data ---

fn mock_api_request_count() -> u64 {
    615
}

fn mock_token_consumption() -> u64 {
    17814528
}

// --- Mock login ---

pub fn mock_login(username: &str, password: &str) -> Result<String, String> {
    if username == "admin" && password == "admin123" {
        Ok("mock-session-token".to_string())
    } else {
        Err("Invalid credentials".to_string())
    }
}

// --- Generic request helper ---

async fn api_get<T: DeserializeOwned>(path: &str) -> Result<T, NetError> {
    let url = format!("{}/admin/{}", get_base_url(), path);
    let resp = Request::get(&url).headers(auth_headers()).send().await?;

    if !resp.ok() {
        return Err(NetError::GlooError(format!(
            "HTTP {} from /admin/{}",
            resp.status(),
            path
        )));
    }

    resp.json().await
}

// --- Auth ---

fn auth_headers() -> Headers {
    let headers = Headers::new();
    headers.set("Authorization", &format!("Bearer {}", ADMIN_TOKEN));
    headers
}

// --- API functions ---

pub async fn fetch_providers() -> Result<Vec<Provider>, NetError> {
    api_get("providers").await
}

pub async fn fetch_models() -> Result<Vec<Model>, NetError> {
    api_get("models").await
}

pub async fn fetch_dashboard() -> Result<DashboardData, NetError> {
    let (providers, models) = futures_util::try_join!(
        api_get::<Vec<Provider>>("providers"),
        api_get::<Vec<Model>>("models"),
    )?;

    Ok(DashboardData {
        providers,
        models,
        api_request_count: mock_api_request_count(),
        token_consumption: mock_token_consumption(),
    })
}
