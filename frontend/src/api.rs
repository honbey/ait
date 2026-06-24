use serde::de::DeserializeOwned;

use gloo_net::Error as NetError;
use gloo_net::http::Request;

use crate::models::{
    ApiKeyListItem, CreateApiKeyResponse, DailyRequests, DailyTokens, DashboardStats,
    LoginResponse, Model, Provider, ProviderTypeInfo,
};

fn get_base_url() -> String {
    sycamore::web::window()
        .location()
        .origin()
        .unwrap_or_default()
}

// --- Core request helpers ---

async fn response_to_error(resp: gloo_net::http::Response) -> NetError {
    let status = resp.status();
    let msg = resp
        .text()
        .await
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(String::from))
        .unwrap_or_else(|| format!("HTTP {status}"));
    NetError::GlooError(msg)
}

async fn api_get<T: DeserializeOwned>(path: &str) -> Result<T, NetError> {
    let url = format!("{}/{}", get_base_url(), path);
    let resp = Request::get(&url).send().await?;
    if resp.ok() {
        resp.json().await
    } else {
        Err(response_to_error(resp).await)
    }
}

async fn api_post<T: DeserializeOwned>(
    path: &str,
    body: &serde_json::Value,
    headers: &[(&str, &str)],
) -> Result<T, NetError> {
    let url = format!("{}/{}", get_base_url(), path);
    let mut req = Request::post(&url).header("Content-Type", "application/json");
    for (k, v) in headers {
        req = req.header(k, v);
    }
    let resp = req
        .body(body.to_string())
        .map_err(|e| NetError::GlooError(e.to_string()))?
        .send()
        .await?;
    if resp.ok() {
        resp.json().await
    } else {
        Err(response_to_error(resp).await)
    }
}

async fn api_put<T: DeserializeOwned>(path: &str, body: &serde_json::Value) -> Result<T, NetError> {
    let url = format!("{}/{}", get_base_url(), path);
    let resp = Request::put(&url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .map_err(|e| NetError::GlooError(e.to_string()))?
        .send()
        .await?;
    if resp.ok() {
        resp.json().await
    } else {
        Err(response_to_error(resp).await)
    }
}

async fn api_delete(path: &str) -> Result<(), NetError> {
    let url = format!("{}/{}", get_base_url(), path);
    let resp = Request::delete(&url).send().await?;
    if resp.ok() {
        Ok(())
    } else {
        Err(response_to_error(resp).await)
    }
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
        "admin/models",
        &serde_json::json!({
            "name": name,
            "provider_id": provider_id,
            "upstream_model": upstream_model,
            "enabled": enabled,
        }),
        &[],
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
        &format!("admin/models/{}", name),
        &serde_json::json!({
            "provider_id": provider_id,
            "upstream_model": upstream_model,
            "enabled": enabled,
        }),
    )
    .await
}

pub async fn delete_model(name: &str) -> Result<(), NetError> {
    api_delete(&format!("admin/models/{}", name)).await
}

// --- API Key CRUD ---

pub async fn fetch_api_keys(username: &str) -> Result<Vec<ApiKeyListItem>, NetError> {
    api_get(&format!("admin/users/{}/api-keys", username)).await
}

pub async fn create_api_key(
    username: &str,
    name: &str,
    expires_at: Option<i64>,
) -> Result<CreateApiKeyResponse, NetError> {
    api_post(
        &format!("admin/users/{}/api-keys", username),
        &serde_json::json!({ "name": name, "expires_at": expires_at }),
        &[],
    )
    .await
}

pub async fn toggle_api_key(
    username: &str,
    key_id: &str,
    enabled: bool,
) -> Result<ApiKeyListItem, NetError> {
    api_put(
        &format!("admin/users/{}/api-keys/{}", username, key_id),
        &serde_json::json!({ "enabled": enabled }),
    )
    .await
}

pub async fn delete_api_key(username: &str, key: &str) -> Result<(), NetError> {
    api_delete(&format!("admin/users/{}/api-keys/{}", username, key)).await
}

// --- Auth ---

pub async fn register_api(
    username: &str,
    password: &str,
    registration_code: &str,
) -> Result<(), NetError> {
    api_post::<serde_json::Value>(
        "admin/register",
        &serde_json::json!({
            "username": username,
            "password": password,
            "registration_code": registration_code,
        }),
        &[],
    )
    .await?;
    Ok(())
}

pub async fn login_api(username: &str, password: &str) -> Result<String, NetError> {
    let resp: LoginResponse = api_post(
        "admin/login",
        &serde_json::json!({ "username": username, "password": password }),
        &[],
    )
    .await?;
    Ok(resp.role)
}

pub async fn check_session() -> Result<Option<(String, String)>, NetError> {
    let json: serde_json::Value = api_get("admin/session").await?;
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
    api_post::<serde_json::Value>("admin/logout", &serde_json::json!({}), &[]).await?;
    Ok(())
}

// --- Data fetchers ---

pub async fn fetch_providers() -> Result<Vec<Provider>, NetError> {
    api_get("admin/providers").await
}

pub async fn fetch_provider_types() -> Result<Vec<ProviderTypeInfo>, NetError> {
    api_get("admin/provider-types").await
}

pub async fn fetch_models() -> Result<Vec<Model>, NetError> {
    api_get("admin/models").await
}

pub async fn fetch_dashboard_stats() -> Result<DashboardStats, NetError> {
    api_get("admin/stats").await
}

pub async fn fetch_daily_requests(days: u32) -> Result<Vec<DailyRequests>, NetError> {
    api_get(&format!("admin/stats/requests?days={}", days)).await
}

pub async fn fetch_daily_tokens(days: u32) -> Result<Vec<DailyTokens>, NetError> {
    api_get(&format!("admin/stats/tokens?days={}", days)).await
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
        "admin/providers",
        &serde_json::json!({
            "name": name,
            "type": provider_type,
            "base_url": base_url,
            "api_key": api_key_value(api_key.as_deref()),
            "enabled": enabled,
        }),
        &[],
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
        &format!("admin/providers/{}", id),
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
    api_delete(&format!("admin/providers/{}", id)).await
}

// --- Text Generation ---

pub async fn generate_completion(
    token: &str,
    model: &str,
    prompt: &str,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    top_p: Option<f32>,
) -> Result<String, NetError> {
    let mut body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "stream": false,
    });
    if let Some(mt) = max_tokens {
        body["max_tokens"] = serde_json::json!(mt);
    }
    if let Some(t) = temperature {
        body["temperature"] = serde_json::json!(t);
    }
    if let Some(p) = top_p {
        body["top_p"] = serde_json::json!(p);
    }
    let auth = format!("Bearer {}", token);
    let json: serde_json::Value =
        api_post("v1/completions", &body, &[("Authorization", &auth)]).await?;
    let text = json
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|c| c.first())
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        .or_else(|| json.get("response").and_then(|r| r.as_str()))
        .unwrap_or("")
        .to_string();
    Ok(text)
}
