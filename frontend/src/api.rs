use serde::de::DeserializeOwned;

use gloo_net::Error as NetError;
use gloo_net::http::{Headers, Request};

use crate::models::{
    ApiKeyListItem, CreateApiKeyResponse, DashboardData, DashboardStats, Model, Provider,
};

/// Get base URL from window.location.origin at runtime
fn get_base_url() -> String {
    sycamore::web::window()
        .location()
        .origin()
        .unwrap_or_default()
}

// --- Generic request helper ---

async fn api_get<T: DeserializeOwned>(path: &str) -> Result<T, NetError> {
    let url = format!("{}/admin/{}", get_base_url(), path);
    let resp = Request::get(&url).send().await?;

    if !resp.ok() {
        return Err(NetError::GlooError(format!(
            "HTTP {} from /admin/{}",
            resp.status(),
            path
        )));
    }

    resp.json().await
}

// --- Model CRUD ---

pub async fn create_model(
    name: &str,
    provider_id: &str,
    upstream_model: &str,
    enabled: bool,
) -> Result<Model, NetError> {
    let url = format!("{}/admin/models", get_base_url());
    let body = serde_json::json!({
        "id": "",
        "name": name,
        "provider_id": provider_id,
        "upstream_model": upstream_model,
        "enabled": enabled,
    });
    let resp = Request::post(&url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .map_err(|e| NetError::GlooError(e.to_string()))?
        .send()
        .await?;

    if !resp.ok() {
        let status = resp.status();
        return Err(NetError::GlooError(format!(
            "HTTP {} creating model",
            status
        )));
    }

    resp.json().await
}

pub async fn update_model(
    name: &str,
    provider_id: &str,
    upstream_model: &str,
    enabled: bool,
) -> Result<Model, NetError> {
    let url = format!("{}/admin/models/{}", get_base_url(), name);
    let body = serde_json::json!({
        "provider_id": provider_id,
        "upstream_model": upstream_model,
        "enabled": enabled,
    });
    let resp = Request::put(&url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .map_err(|e| NetError::GlooError(e.to_string()))?
        .send()
        .await?;

    if !resp.ok() {
        let status = resp.status();
        return Err(NetError::GlooError(format!(
            "HTTP {} updating model",
            status
        )));
    }

    resp.json().await
}

pub async fn delete_model(name: &str) -> Result<(), NetError> {
    let url = format!("{}/admin/models/{}", get_base_url(), name);
    let resp = Request::delete(&url).send().await?;

    if !resp.ok() {
        let status = resp.status();
        return Err(NetError::GlooError(format!(
            "HTTP {} deleting model",
            status
        )));
    }

    Ok(())
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
    let url = format!("{}/admin/users/{}/api-keys", get_base_url(), username);
    let body = serde_json::json!({ "name": name, "expires_at": expires_at });
    let resp = Request::post(&url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .map_err(|e| NetError::GlooError(e.to_string()))?
        .send()
        .await?;

    if !resp.ok() {
        let status = resp.status();
        return Err(NetError::GlooError(format!(
            "HTTP {} creating API key",
            status
        )));
    }

    resp.json().await
}

pub async fn toggle_api_key(
    username: &str,
    key_id: &str,
    enabled: bool,
) -> Result<ApiKeyListItem, NetError> {
    let url = format!(
        "{}/admin/users/{}/api-keys/{}",
        get_base_url(),
        username,
        key_id
    );
    let body = serde_json::json!({ "enabled": enabled });
    let resp = Request::put(&url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .map_err(|e| NetError::GlooError(e.to_string()))?
        .send()
        .await?;

    if !resp.ok() {
        let status = resp.status();
        return Err(NetError::GlooError(format!(
            "HTTP {} toggling API key",
            status
        )));
    }

    resp.json().await
}

pub async fn delete_api_key(username: &str, key: &str) -> Result<(), NetError> {
    let url = format!(
        "{}/admin/users/{}/api-keys/{}",
        get_base_url(),
        username,
        key
    );
    let resp = Request::delete(&url).send().await?;

    if !resp.ok() {
        let status = resp.status();
        return Err(NetError::GlooError(format!(
            "HTTP {} deleting API key",
            status
        )));
    }

    Ok(())
}

// --- Auth API ---

pub async fn register_api(
    username: &str,
    password: &str,
    registration_code: &str,
) -> Result<(), String> {
    let url = format!("{}/admin/register", get_base_url());
    let body = serde_json::json!({
        "username": username,
        "password": password,
        "registration_code": registration_code
    });
    let req = Request::post(&url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .map_err(|e| e.to_string())?;
    let resp = req.send().await.map_err(|e| e.to_string())?;

    if !resp.ok() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if let Some(msg) = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(String::from))
        {
            return Err(msg);
        }
        return Err(format!("Registration failed (HTTP {})", status));
    }

    Ok(())
}

pub async fn login_api(username: &str, password: &str) -> Result<(), String> {
    let url = format!("{}/admin/login", get_base_url());
    let body = serde_json::json!({ "username": username, "password": password });
    let req = Request::post(&url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .map_err(|e| e.to_string())?;
    let resp = req.send().await.map_err(|e| e.to_string())?;

    if !resp.ok() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if let Some(msg) = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(String::from))
        {
            return Err(msg);
        }
        return Err(format!("Login failed (HTTP {})", status));
    }

    Ok(())
}

pub async fn check_session() -> Result<Option<(String, String)>, String> {
    let url = format!("{}/admin/session", get_base_url());
    let resp = Request::get(&url)
        .headers(Headers::new())
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let authenticated = json
        .get("authenticated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if authenticated {
        let username = json
            .get("username")
            .and_then(|v| v.as_str())
            .map(String::from);
        let role = json.get("role").and_then(|v| v.as_str()).map(String::from);
        match (username, role) {
            (Some(u), Some(r)) => Ok(Some((u, r))),
            _ => Ok(None),
        }
    } else {
        Ok(None)
    }
}

pub async fn logout_api() -> Result<(), String> {
    let url = format!("{}/admin/logout", get_base_url());
    let req = Request::post(&url).body("").map_err(|e| e.to_string())?;
    req.send().await.map_err(|e| e.to_string())?;
    Ok(())
}

// --- API functions ---

pub async fn fetch_providers() -> Result<Vec<Provider>, NetError> {
    api_get("providers").await
}

pub async fn fetch_models() -> Result<Vec<Model>, NetError> {
    api_get("models").await
}

pub async fn fetch_dashboard() -> Result<DashboardData, NetError> {
    let (stats,) = futures_util::try_join!(api_get::<DashboardStats>("stats"),)?;

    Ok(DashboardData {
        provider_count: stats.provider_count,
        model_count: stats.model_count,
        api_request_count: stats.api_request_count,
        token_consumption: stats.token_consumption,
    })
}

pub async fn update_provider(
    id: &str,
    name: &str,
    provider_type: &str,
    base_url: &str,
    api_key: Option<String>,
    enabled: bool,
) -> Result<Provider, NetError> {
    let url = format!("{}/admin/providers/{}", get_base_url(), id);
    let api_key_value = api_key
        .as_deref()
        .map(|k| serde_json::Value::String(k.to_string()))
        .unwrap_or(serde_json::Value::Null);
    let body = serde_json::json!({
        "id": id,
        "name": name,
        "type": provider_type,
        "base_url": base_url,
        "api_key": api_key_value,
        "enabled": enabled,
    });
    let resp = Request::put(&url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .map_err(|e| NetError::GlooError(e.to_string()))?
        .send()
        .await?;

    if !resp.ok() {
        let status = resp.status();
        return Err(NetError::GlooError(format!(
            "HTTP {} updating provider",
            status
        )));
    }

    resp.json().await
}

pub async fn delete_provider(id: &str) -> Result<(), NetError> {
    let url = format!("{}/admin/providers/{}", get_base_url(), id);
    let resp = Request::delete(&url).send().await?;

    if !resp.ok() {
        let status = resp.status();
        return Err(NetError::GlooError(format!(
            "HTTP {} deleting provider",
            status
        )));
    }

    Ok(())
}

pub async fn create_provider(
    name: &str,
    provider_type: &str,
    base_url: &str,
    api_key: Option<String>,
    enabled: bool,
) -> Result<Provider, NetError> {
    let url = format!("{}/admin/providers", get_base_url());
    let api_key_value = api_key
        .as_deref()
        .map(|k| serde_json::Value::String(k.to_string()))
        .unwrap_or(serde_json::Value::Null);
    let body = serde_json::json!({
        "id": "",
        "name": name,
        "type": provider_type,
        "base_url": base_url,
        "api_key": api_key_value,
        "enabled": enabled,
    });
    let resp = Request::post(&url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .map_err(|e| NetError::GlooError(e.to_string()))?
        .send()
        .await?;

    if !resp.ok() {
        let status = resp.status();
        return Err(NetError::GlooError(format!(
            "HTTP {} creating provider",
            status
        )));
    }

    resp.json().await
}
