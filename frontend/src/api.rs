use serde::de::DeserializeOwned;

use gloo_net::http::{Headers, Request};
use gloo_net::Error as NetError;

use crate::models::{DashboardData, Model, Provider};

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

// --- Auth headers (session via HttpOnly cookie; static Bearer for CLI/scripts) ---

fn auth_headers() -> Headers {
    Headers::new()
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
