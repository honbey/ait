use std::cell::Cell;
use std::cell::OnceCell;
use std::cell::RefCell;
use std::collections::HashMap;

use gloo_net::Error as NetError;
use gloo_net::http::Request;
use leptos::prelude::*;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::auth::{AuthContext, AuthStatus};
use crate::components::toast::ToastManager;
use crate::i18n::{I18n, K};
use reactive_stores::{Patch, Store};

thread_local! {
    static SUPPRESS_401: Cell<bool> = const { Cell::new(false) };
    static BASE_URL: OnceCell<String> = const { OnceCell::new() };
}

/// 30s cap for admin API requests. Streaming /v1 calls bypass this.
const REQUEST_TIMEOUT_MS: u32 = 30_000;

fn timeout_signal() -> web_sys::AbortSignal {
    web_sys::AbortSignal::timeout_with_u32(REQUEST_TIMEOUT_MS)
}

struct CachedResponse {
    json: String,
    cached_at: f64,
}

thread_local! {
    static FETCH_CACHE: RefCell<HashMap<String, CachedResponse>> = RefCell::new(HashMap::new());
}

const CACHE_TTL_MS: f64 = 120_000.0;
const MAX_CACHE_ENTRIES: usize = 5;

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderTypeInfo {
    #[serde(rename = "type")]
    pub provider_type: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Default, Deserialize, Store, Patch)]
pub struct Provider {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub provider_type: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub enabled: bool,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
}

struct Suppress401Guard;

impl Suppress401Guard {
    fn new() -> Self {
        SUPPRESS_401.set(true);
        Suppress401Guard
    }
}

impl Drop for Suppress401Guard {
    fn drop(&mut self) {
        SUPPRESS_401.set(false);
    }
}

pub fn clear_cache() {
    FETCH_CACHE.with(|c| c.borrow_mut().clear());
}

fn handle_401(status: u16) {
    if status != 401 || SUPPRESS_401.get() {
        return;
    }
    clear_cache();
    if let Some(auth) = use_context::<AuthContext>() {
        auth.authenticated.set(AuthStatus::NotAuthenticated);
        if let (Some(toast), Some(i18n)) = (use_context::<ToastManager>(), use_context::<I18n>()) {
            toast.error(i18n.t_untracked(K::SessionExpired));
        }
    }
}

pub fn get_base_url() -> String {
    BASE_URL.with(|cell| {
        cell.get_or_init(|| {
            web_sys::window()
                .and_then(|w| w.location().origin().ok())
                .unwrap_or_default()
        })
        .clone()
    })
}

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

async fn api_post<T: DeserializeOwned>(
    path: &str,
    body: &serde_json::Value,
) -> Result<T, NetError> {
    let url = format!("{}/{}", get_base_url(), path);
    let resp = Request::post(&url)
        .header("Content-Type", "application/json")
        .abort_signal(Some(&timeout_signal()))
        .body(body.to_string())
        .map_err(|e| NetError::GlooError(e.to_string()))?
        .send()
        .await?;
    if resp.ok() {
        resp.json().await
    } else {
        let status = resp.status();
        let err = response_to_error(resp).await;
        handle_401(status);
        Err(err)
    }
}

async fn api_put<T: DeserializeOwned>(path: &str, body: &serde_json::Value) -> Result<T, NetError> {
    let url = format!("{}/{}", get_base_url(), path);
    let resp = Request::put(&url)
        .header("Content-Type", "application/json")
        .abort_signal(Some(&timeout_signal()))
        .body(body.to_string())
        .map_err(|e| NetError::GlooError(e.to_string()))?
        .send()
        .await?;
    if resp.ok() {
        resp.json().await
    } else {
        let status = resp.status();
        let err = response_to_error(resp).await;
        handle_401(status);
        Err(err)
    }
}

async fn api_delete(path: &str) -> Result<(), NetError> {
    let url = format!("{}/{}", get_base_url(), path);
    let resp = Request::delete(&url)
        .abort_signal(Some(&timeout_signal()))
        .send()
        .await?;
    if resp.ok() {
        Ok(())
    } else {
        let status = resp.status();
        let err = response_to_error(resp).await;
        handle_401(status);
        Err(err)
    }
}

async fn api_get<T: DeserializeOwned>(path: &str) -> Result<T, NetError> {
    let url = format!("{}/{}", get_base_url(), path);
    let resp = Request::get(&url)
        .abort_signal(Some(&timeout_signal()))
        .send()
        .await?;
    if resp.ok() {
        resp.json().await
    } else {
        let status = resp.status();
        let err = response_to_error(resp).await;
        handle_401(status);
        Err(err)
    }
}

async fn api_get_cached<T: DeserializeOwned>(path: &str, force: bool) -> Result<T, NetError> {
    let key = path.to_string();

    if !force
        && let Some(cached) = FETCH_CACHE.with(|c| {
            let map = c.borrow();
            map.get(&key).and_then(|entry| {
                if js_sys::Date::now() - entry.cached_at < CACHE_TTL_MS {
                    Some(entry.json.clone())
                } else {
                    None
                }
            })
        })
    {
        return serde_json::from_str(&cached).map_err(|e| NetError::GlooError(e.to_string()));
    }

    let url = format!("{}/{}", get_base_url(), path);
    let resp = Request::get(&url)
        .abort_signal(Some(&timeout_signal()))
        .send()
        .await?;
    if !resp.ok() {
        let status = resp.status();
        let err = response_to_error(resp).await;
        handle_401(status);
        return Err(err);
    }
    let text = resp.text().await?;
    let result: T = serde_json::from_str(&text).map_err(|e| NetError::GlooError(e.to_string()))?;

    FETCH_CACHE.with(|c| {
        let mut map = c.borrow_mut();
        map.insert(
            key,
            CachedResponse {
                json: text,
                cached_at: js_sys::Date::now(),
            },
        );
        if map.len() > MAX_CACHE_ENTRIES
            && let Some(oldest_key) = map
                .iter()
                .min_by(|(_, a), (_, b)| a.cached_at.total_cmp(&b.cached_at))
                .map(|(k, _)| k.clone())
        {
            map.remove(&oldest_key);
        }
    });

    Ok(result)
}

pub async fn login_api(username: &str, password: &str) -> Result<(), NetError> {
    let _guard = Suppress401Guard::new();
    api_post::<serde_json::Value>(
        "auth/login",
        &serde_json::json!({ "username": username, "password": password }),
    )
    .await?;
    Ok(())
}

pub async fn logout_api() -> Result<(), NetError> {
    let _guard = Suppress401Guard::new();
    api_post::<serde_json::Value>("auth/logout", &serde_json::json!({})).await?;
    Ok(())
}

pub async fn change_password_api(
    current_password: &str,
    new_password: &str,
) -> Result<(), NetError> {
    let username = use_context::<AuthContext>()
        .and_then(|auth| auth.username.get_untracked())
        .ok_or_else(|| NetError::GlooError("not logged in".into()))?;
    api_put::<serde_json::Value>(
        &format!("api/users/{}/password", username),
        &serde_json::json!({
            "current_password": current_password,
            "new_password": new_password,
        }),
    )
    .await?;
    Ok(())
}

pub async fn check_session() -> Result<Option<String>, NetError> {
    let _guard = Suppress401Guard::new();
    let json: serde_json::Value = api_get("auth/session").await?;
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
    Ok(username)
}

pub async fn fetch_providers() -> Result<Vec<Provider>, NetError> {
    api_get("api/providers").await
}

pub async fn fetch_provider_types() -> Result<Vec<ProviderTypeInfo>, NetError> {
    api_get("api/provider-types").await
}

pub async fn create_provider(
    name: &str,
    provider_type: &str,
    base_url: &str,
    api_key: Option<&str>,
    enabled: bool,
) -> Result<Provider, NetError> {
    api_post(
        "api/providers",
        &serde_json::json!({
            "name": name,
            "type": provider_type,
            "base_url": base_url,
            "api_key": api_key,
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
    api_key: Option<&str>,
    enabled: bool,
) -> Result<Provider, NetError> {
    api_put(
        &format!("api/providers/{}", id),
        &serde_json::json!({
            "name": name,
            "type": provider_type,
            "base_url": base_url,
            "api_key": api_key,
            "enabled": enabled,
        }),
    )
    .await
}

pub async fn delete_provider(id: &str) -> Result<(), NetError> {
    api_delete(&format!("api/providers/{}", id)).await
}

#[derive(Debug, Clone, Deserialize)]
pub struct OverviewStats {
    pub provider_count: u64,
    pub model_count: u64,
    pub api_request_count: u64,
    pub token_consumption: u64,
    pub rpm: f64,
    pub tpm: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelDistEntry {
    pub model: String,
    pub count: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenDistEntry {
    pub category: String,
    pub count: u64,
}

#[derive(Debug, Clone, Default, Deserialize, Store, Patch)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub provider_id: String,
    pub upstream_model: String,
    pub enabled: bool,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
}

pub async fn fetch_models() -> Result<Vec<Model>, NetError> {
    api_get("api/models").await
}

pub async fn create_model(
    name: &str,
    provider_id: &str,
    upstream_model: &str,
    enabled: bool,
) -> Result<Model, NetError> {
    api_post(
        "api/models",
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
        &format!("api/models/{}", name),
        &serde_json::json!({
            "provider_id": provider_id,
            "upstream_model": upstream_model,
            "enabled": enabled,
        }),
    )
    .await
}

pub async fn delete_model(name: &str) -> Result<(), NetError> {
    api_delete(&format!("api/models/{}", name)).await
}

#[derive(Debug, Clone, Deserialize)]
pub struct BucketEntry {
    pub timestamp: i64,
    pub count: u64,
}

// --- API Key Management ---

#[derive(Debug, Clone, Default, Deserialize, Store, Patch)]
pub struct ApiKey {
    pub id: String,
    #[serde(rename = "key")]
    pub display: String,
    pub name: String,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
    pub enabled: bool,
    #[serde(default)]
    pub expires_at: Option<i64>,
}

pub async fn fetch_api_keys(username: &str) -> Result<Vec<ApiKey>, NetError> {
    api_get(&format!("api/users/{}/api-keys", username)).await
}

pub async fn create_api_key(
    username: &str,
    name: &str,
    expires_at: Option<i64>,
) -> Result<ApiKey, NetError> {
    api_post(
        &format!("api/users/{}/api-keys", username),
        &serde_json::json!({ "name": name, "expires_at": expires_at }),
    )
    .await
}

pub async fn update_api_key(
    username: &str,
    key_id: &str,
    name: Option<&str>,
    expires_at: Option<i64>,
    enabled: Option<bool>,
) -> Result<ApiKey, NetError> {
    let mut body = serde_json::json!({});
    if let Some(n) = name {
        body["name"] = serde_json::json!(n);
    }
    if let Some(n) = expires_at {
        body["expires_at"] = serde_json::json!(n);
    }
    if let Some(e) = enabled {
        body["enabled"] = serde_json::json!(e);
    }
    api_put(
        &format!("api/users/{}/api-keys/{}", username, key_id),
        &body,
    )
    .await
}

pub async fn delete_api_key(username: &str, key_id: &str) -> Result<(), NetError> {
    api_delete(&format!("api/users/{}/api-keys/{}", username, key_id)).await
}

pub async fn fetch_overview_stats(
    start_ts: i64,
    end_ts: i64,
    force: bool,
) -> Result<OverviewStats, NetError> {
    api_get_cached(
        &format!("api/stats?start_ts={}&end_ts={}", start_ts, end_ts),
        force,
    )
    .await
}

pub async fn fetch_model_dist(
    start_ts: i64,
    end_ts: i64,
    force: bool,
) -> Result<Vec<ModelDistEntry>, NetError> {
    api_get_cached(
        &format!(
            "api/data/model-dist?start_ts={}&end_ts={}",
            start_ts, end_ts
        ),
        force,
    )
    .await
}

pub async fn fetch_token_dist(
    start_ts: i64,
    end_ts: i64,
    force: bool,
) -> Result<Vec<TokenDistEntry>, NetError> {
    api_get_cached(
        &format!(
            "api/data/token-dist?start_ts={}&end_ts={}",
            start_ts, end_ts
        ),
        force,
    )
    .await
}

pub async fn fetch_request_buckets(
    start_ts: i64,
    end_ts: i64,
    force: bool,
) -> Result<Vec<BucketEntry>, NetError> {
    api_get_cached(
        &format!("api/data/requests?start_ts={}&end_ts={}", start_ts, end_ts),
        force,
    )
    .await
}

pub async fn fetch_token_buckets(
    start_ts: i64,
    end_ts: i64,
    force: bool,
) -> Result<Vec<BucketEntry>, NetError> {
    api_get_cached(
        &format!("api/data/tokens?start_ts={}&end_ts={}", start_ts, end_ts),
        force,
    )
    .await
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub struct ProxyLogEntryResponse {
    pub timestamp: i64,
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
    pub endpoint: String,
    pub is_streaming: bool,
    pub time_to_first_token_ms: Option<i64>,
    pub upstream_model: String,
    pub provider_type: String,
    pub response_body_size: Option<i64>,
    pub error_message: Option<String>,
    pub client_ip: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub total: u64,
    pub page: u64,
    pub per_page: u64,
}

fn push_qs(parts: &mut Vec<String>, key: &str, value: Option<impl std::fmt::Display>) {
    if let Some(v) = value {
        let s = v.to_string();
        let encoded = js_sys::encode_uri_component(&s);
        parts.push(format!("{}={}", key, encoded.as_string().unwrap_or(s)));
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn fetch_proxy_logs(
    page: u64,
    per_page: u64,
    start_ts: Option<i64>,
    end_ts: Option<i64>,
    provider_name: Option<String>,
    model_name: Option<String>,
    api_key_name: Option<String>,
    client_ip: Option<String>,
    status: Option<String>,
    endpoint: Option<String>,
    is_streaming: Option<bool>,
) -> Result<PaginatedResponse<ProxyLogEntryResponse>, NetError> {
    let mut parts = vec![format!("page={}", page), format!("per_page={}", per_page)];
    push_qs(&mut parts, "start_ts", start_ts);
    push_qs(&mut parts, "end_ts", end_ts);
    push_qs(&mut parts, "provider_name", provider_name);
    push_qs(&mut parts, "model_name", model_name);
    push_qs(&mut parts, "api_key_name", api_key_name);
    push_qs(&mut parts, "client_ip", client_ip);
    push_qs(&mut parts, "status", status);
    push_qs(&mut parts, "endpoint", endpoint);
    push_qs(&mut parts, "is_streaming", is_streaming);
    let path = format!("api/data/proxy-log?{}", parts.join("&"));
    api_get(&path).await
}
