use serde::de::DeserializeOwned;

use gloo_net::Error as NetError;
use gloo_net::http::Request;

use crate::models::{
    ApiKeyListItem, CreateApiKeyResponse, DashboardData, DashboardStats, Model, Provider,
};

fn get_base_url() -> String {
    sycamore::web::window()
        .location()
        .origin()
        .unwrap_or_default()
}

// --- Core request helpers ---

fn check_response(resp: &gloo_net::http::Response, context: &str) -> Result<(), NetError> {
    if resp.ok() {
        Ok(())
    } else {
        Err(NetError::GlooError(format!(
            "HTTP {}: {}",
            resp.status(),
            context
        )))
    }
}

async fn api_get<T: DeserializeOwned>(path: &str) -> Result<T, NetError> {
    let url = format!("{}/admin/{}", get_base_url(), path);
    let resp = Request::get(&url).send().await?;
    check_response(&resp, path)?;
    resp.json().await
}

async fn api_post<T: DeserializeOwned>(
    path: &str,
    body: &serde_json::Value,
) -> Result<T, NetError> {
    let url = format!("{}/admin/{}", get_base_url(), path);
    let resp = Request::post(&url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .map_err(|e| NetError::GlooError(e.to_string()))?
        .send()
        .await?;
    check_response(&resp, path)?;
    resp.json().await
}

async fn api_put<T: DeserializeOwned>(path: &str, body: &serde_json::Value) -> Result<T, NetError> {
    let url = format!("{}/admin/{}", get_base_url(), path);
    let resp = Request::put(&url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .map_err(|e| NetError::GlooError(e.to_string()))?
        .send()
        .await?;
    check_response(&resp, path)?;
    resp.json().await
}

async fn api_delete(path: &str) -> Result<(), NetError> {
    let url = format!("{}/admin/{}", get_base_url(), path);
    let resp = Request::delete(&url).send().await?;
    check_response(&resp, path)?;
    Ok(())
}

// POST with JSON error body parsing (for auth endpoints that return structured errors)
async fn api_post_auth(path: &str, body: &serde_json::Value) -> Result<(), NetError> {
    let url = format!("{}/admin/{}", get_base_url(), path);
    let resp = Request::post(&url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .map_err(|e| NetError::GlooError(e.to_string()))?
        .send()
        .await?;
    if !resp.ok() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let msg = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(String::from))
            .unwrap_or_else(|| format!("HTTP {}", status));
        return Err(NetError::GlooError(msg));
    }
    Ok(())
}

fn api_key_value(api_key: Option<&str>) -> serde_json::Value {
    api_key
        .map(|k| serde_json::Value::String(k.to_string()))
        .unwrap_or(serde_json::Value::Null)
}

// --- Model CRUD ---

pub async fn create_model(
    name: &str,
    provider_id: &str,
    upstream_model: &str,
    enabled: bool,
) -> Result<Model, NetError> {
    api_post(
        "models",
        &serde_json::json!({
            "name": name,
            "provider_id": provider_id,
            "upstream_model": upstream_model,
            "enabled": enabled,
        }),
    )
    .await
}

pub async fn update_model(
    name: &str,
    provider_id: &str,
    upstream_model: &str,
    enabled: bool,
) -> Result<Model, NetError> {
    api_put(
        &format!("models/{}", name),
        &serde_json::json!({
            "provider_id": provider_id,
            "upstream_model": upstream_model,
            "enabled": enabled,
        }),
    )
    .await
}

pub async fn delete_model(name: &str) -> Result<(), NetError> {
    api_delete(&format!("models/{}", name)).await
}

// --- API Key CRUD ---

pub async fn fetch_api_keys(username: &str) -> Result<Vec<ApiKeyListItem>, NetError> {
    api_get(&format!("users/{}/api-keys", username)).await
}

pub async fn create_api_key(
    username: &str,
    name: &str,
    expires_at: Option<i64>,
) -> Result<CreateApiKeyResponse, NetError> {
    api_post(
        &format!("users/{}/api-keys", username),
        &serde_json::json!({ "name": name, "expires_at": expires_at }),
    )
    .await
}

pub async fn toggle_api_key(
    username: &str,
    key_id: &str,
    enabled: bool,
) -> Result<ApiKeyListItem, NetError> {
    api_put(
        &format!("users/{}/api-keys/{}", username, key_id),
        &serde_json::json!({ "enabled": enabled }),
    )
    .await
}

pub async fn delete_api_key(username: &str, key: &str) -> Result<(), NetError> {
    api_delete(&format!("users/{}/api-keys/{}", username, key)).await
}

// --- Auth ---

pub async fn register_api(
    username: &str,
    password: &str,
    registration_code: &str,
) -> Result<(), NetError> {
    api_post_auth(
        "register",
        &serde_json::json!({
            "username": username,
            "password": password,
            "registration_code": registration_code,
        }),
    )
    .await
}

pub async fn login_api(username: &str, password: &str) -> Result<(), NetError> {
    api_post_auth(
        "login",
        &serde_json::json!({ "username": username, "password": password }),
    )
    .await
}

pub async fn check_session() -> Result<Option<(String, String)>, NetError> {
    let url = format!("{}/admin/session", get_base_url());
    let resp = Request::get(&url).send().await?;
    check_response(&resp, "session")?;

    let json: serde_json::Value = resp.json().await?;
    let authenticated = json
        .get("authenticated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !authenticated {
        return Ok(None);
    }
    let username = json
        .get("username")
        .and_then(|v| v.as_str())
        .map(String::from);
    let role = json.get("role").and_then(|v| v.as_str()).map(String::from);
    Ok(match (username, role) {
        (Some(u), Some(r)) => Some((u, r)),
        _ => None,
    })
}

pub async fn logout_api() -> Result<(), NetError> {
    let url = format!("{}/admin/logout", get_base_url());
    let resp = Request::post(&url)
        .body("")
        .map_err(|e| NetError::GlooError(e.to_string()))?
        .send()
        .await?;
    check_response(&resp, "logout")?;
    Ok(())
}

// --- Data fetchers ---

pub async fn fetch_providers() -> Result<Vec<Provider>, NetError> {
    api_get("providers").await
}

pub async fn fetch_models() -> Result<Vec<Model>, NetError> {
    api_get("models").await
}

pub async fn fetch_dashboard() -> Result<DashboardData, NetError> {
    let stats = api_get::<DashboardStats>("stats").await?;
    Ok(DashboardData {
        provider_count: stats.provider_count,
        model_count: stats.model_count,
        api_request_count: stats.api_request_count,
        token_consumption: stats.token_consumption,
    })
}

// --- Provider CRUD ---

pub async fn create_provider(
    name: &str,
    provider_type: &str,
    base_url: &str,
    api_key: Option<String>,
    enabled: bool,
) -> Result<Provider, NetError> {
    api_post(
        "providers",
        &serde_json::json!({
            "name": name,
            "type": provider_type,
            "base_url": base_url,
            "api_key": api_key_value(api_key.as_deref()),
            "enabled": enabled,
        }),
    )
    .await
}

pub async fn update_provider(
    id: &str,
    name: &str,
    provider_type: &str,
    base_url: &str,
    api_key: Option<String>,
    enabled: bool,
) -> Result<Provider, NetError> {
    api_put(
        &format!("providers/{}", id),
        &serde_json::json!({
            "name": name,
            "type": provider_type,
            "base_url": base_url,
            "api_key": api_key_value(api_key.as_deref()),
            "enabled": enabled,
        }),
    )
    .await
}

pub async fn delete_provider(id: &str) -> Result<(), NetError> {
    api_delete(&format!("providers/{}", id)).await
}
