use std::cell::OnceCell;
use std::cell::RefCell;
use std::collections::HashMap;

use gloo_net::Error as NetError;
use gloo_net::http::Request;
use leptos::prelude::*;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::components::toast::ToastManager;
use crate::i18n::{I18n, K};
use reactive_stores::{Patch, Store};

thread_local! {
    static BASE_URL: OnceCell<String> = const { OnceCell::new() };
}

/// 30s cap for admin API requests. Streaming /v1 calls bypass this.
const REQUEST_TIMEOUT_MS: u32 = 30_000;

/// `AbortSignal.timeout` is unavailable on Safari < 16 and Firefox < 100, where
/// calling it throws and would fail every request. Detect it once and degrade
/// to no client-side timeout instead of breaking the whole console.
fn timeout_signal() -> Option<web_sys::AbortSignal> {
    let ctor = js_sys::Reflect::get(&js_sys::global(), &"AbortSignal".into()).ok()?;
    let supported = js_sys::Reflect::get(&ctor, &"timeout".into())
        .map(|value| !value.is_undefined())
        .unwrap_or(false);
    if !supported {
        return None;
    }
    Some(web_sys::AbortSignal::timeout_with_u32(REQUEST_TIMEOUT_MS))
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

pub fn clear_cache() {
    FETCH_CACHE.with(|c| c.borrow_mut().clear());
}

fn handle_401(status: u16) {
    if status != 401 {
        return;
    }
    clear_cache();
    if let (Some(toast), Some(i18n)) = (use_context::<ToastManager>(), use_context::<I18n>()) {
        toast.error(i18n.t_untracked(K::ApiKeyExpired));
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

/// Error returned by every admin API call.
///
/// `request_id` mirrors the id the backend injects into error bodies (and the
/// `x-request-id` header), so a failed request can be matched against the
/// server log. `Display` renders the message alone — the id is surfaced
/// separately by the UI so it stays copyable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiError {
    pub message: String,
    pub request_id: Option<String>,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<NetError> for ApiError {
    fn from(e: NetError) -> Self {
        Self {
            message: e.to_string(),
            request_id: None,
        }
    }
}

async fn response_to_error(resp: gloo_net::http::Response) -> ApiError {
    let status = resp.status();
    let parsed = resp
        .text()
        .await
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    let message = parsed
        .as_ref()
        .and_then(|v| v.get("message"))
        .and_then(|m| m.as_str())
        .map(String::from)
        .unwrap_or_else(|| format!("HTTP {status}"));
    let request_id = parsed
        .as_ref()
        .and_then(|v| v.get("request_id"))
        .and_then(|id| id.as_str())
        .map(String::from);
    ApiError {
        message,
        request_id,
    }
}

async fn api_post<T: DeserializeOwned>(
    path: &str,
    body: &serde_json::Value,
) -> Result<T, ApiError> {
    let url = format!("{}/{}", get_base_url(), path);
    let signal = timeout_signal();
    let resp = Request::post(&url)
        .header("Content-Type", "application/json")
        .abort_signal(signal.as_ref())
        .body(body.to_string())
        .map_err(|e| NetError::GlooError(e.to_string()))?
        .send()
        .await?;
    if resp.ok() {
        // A successful mutation invalidates the read cache so the next read
        // observes it.
        FETCH_CACHE.with(|c| c.borrow_mut().clear());
        resp.json().await.map_err(ApiError::from)
    } else {
        let status = resp.status();
        let err = response_to_error(resp).await;
        handle_401(status);
        Err(err)
    }
}

async fn api_put<T: DeserializeOwned>(path: &str, body: &serde_json::Value) -> Result<T, ApiError> {
    let url = format!("{}/{}", get_base_url(), path);
    let signal = timeout_signal();
    let resp = Request::put(&url)
        .header("Content-Type", "application/json")
        .abort_signal(signal.as_ref())
        .body(body.to_string())
        .map_err(|e| NetError::GlooError(e.to_string()))?
        .send()
        .await?;
    if resp.ok() {
        FETCH_CACHE.with(|c| c.borrow_mut().clear());
        resp.json().await.map_err(ApiError::from)
    } else {
        let status = resp.status();
        let err = response_to_error(resp).await;
        handle_401(status);
        Err(err)
    }
}

async fn api_delete(path: &str) -> Result<(), ApiError> {
    let url = format!("{}/{}", get_base_url(), path);
    let signal = timeout_signal();
    let resp = Request::delete(&url)
        .abort_signal(signal.as_ref())
        .send()
        .await?;
    if resp.ok() {
        FETCH_CACHE.with(|c| c.borrow_mut().clear());
        Ok(())
    } else {
        let status = resp.status();
        let err = response_to_error(resp).await;
        handle_401(status);
        Err(err)
    }
}

async fn api_get<T: DeserializeOwned>(path: &str) -> Result<T, ApiError> {
    let url = format!("{}/{}", get_base_url(), path);
    let signal = timeout_signal();
    let resp = Request::get(&url)
        .abort_signal(signal.as_ref())
        .send()
        .await?;
    if resp.ok() {
        resp.json().await.map_err(ApiError::from)
    } else {
        let status = resp.status();
        let err = response_to_error(resp).await;
        handle_401(status);
        Err(err)
    }
}

async fn api_get_cached<T: DeserializeOwned>(path: &str, force: bool) -> Result<T, ApiError> {
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
        return serde_json::from_str(&cached)
            .map_err(|e| ApiError::from(NetError::GlooError(e.to_string())));
    }

    let url = format!("{}/{}", get_base_url(), path);
    let signal = timeout_signal();
    let resp = Request::get(&url)
        .abort_signal(signal.as_ref())
        .send()
        .await?;
    if !resp.ok() {
        let status = resp.status();
        let err = response_to_error(resp).await;
        handle_401(status);
        return Err(err);
    }
    let text = resp.text().await?;
    let result: T = serde_json::from_str(&text)
        .map_err(|e| ApiError::from(NetError::GlooError(e.to_string())))?;

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

pub async fn fetch_providers() -> Result<Vec<Provider>, ApiError> {
    api_get("api/providers").await
}

pub async fn fetch_provider_types() -> Result<Vec<ProviderTypeInfo>, ApiError> {
    api_get("api/provider-types").await
}

pub async fn create_provider(
    name: &str,
    provider_type: &str,
    base_url: &str,
    api_key: Option<&str>,
    enabled: bool,
) -> Result<Provider, ApiError> {
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
) -> Result<Provider, ApiError> {
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

pub async fn delete_provider(id: &str) -> Result<(), ApiError> {
    api_delete(&format!("api/providers/{}", id)).await
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct OverviewStats {
    #[serde(default)]
    pub range_start: i64,
    #[serde(default)]
    pub range_end: i64,
    pub provider_count: u64,
    pub model_count: u64,
    pub api_request_count: u64,
    pub token_consumption: u64,
    pub rpm: f64,
    pub tpm: f64,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub request_buckets: Vec<BucketEntry>,
    #[serde(default)]
    pub token_buckets: Vec<BucketEntry>,
    #[serde(default)]
    pub model_dist: Vec<ModelDistEntry>,
    #[serde(default)]
    pub token_dist: Vec<TokenDistEntry>,
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

pub async fn fetch_models() -> Result<Vec<Model>, ApiError> {
    api_get("api/models").await
}

pub async fn create_model(
    name: &str,
    provider_id: &str,
    upstream_model: &str,
    enabled: bool,
) -> Result<Model, ApiError> {
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
) -> Result<Model, ApiError> {
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

pub async fn delete_model(name: &str) -> Result<(), ApiError> {
    api_delete(&format!("api/models/{}", name)).await
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
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

pub async fn fetch_api_keys() -> Result<Vec<ApiKey>, ApiError> {
    api_get("api/api-keys").await
}

pub async fn create_api_key(name: &str, expires_at: Option<i64>) -> Result<ApiKey, ApiError> {
    api_post(
        "api/api-keys",
        &serde_json::json!({ "name": name, "expires_at": expires_at }),
    )
    .await
}

pub async fn update_api_key(
    key_id: &str,
    name: Option<&str>,
    expires_at: Option<i64>,
    enabled: Option<bool>,
) -> Result<ApiKey, ApiError> {
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
    api_put(&format!("api/api-keys/{}", key_id), &body).await
}

pub async fn delete_api_key(key_id: &str) -> Result<(), ApiError> {
    api_delete(&format!("api/api-keys/{}", key_id)).await
}

pub async fn fetch_overview_stats(
    start_ts: i64,
    end_ts: i64,
    force: bool,
) -> Result<OverviewStats, ApiError> {
    api_get_cached(
        &format!("api/stats?start_ts={}&end_ts={}", start_ts, end_ts),
        force,
    )
    .await
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
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
    pub request_id: String,
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
) -> Result<PaginatedResponse<ProxyLogEntryResponse>, ApiError> {
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
