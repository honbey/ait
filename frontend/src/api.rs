use serde::de::DeserializeOwned;

use gloo_net::Error as NetError;
use gloo_net::http::Request;

use crate::models::{
    ApiKeyListItem, BucketEntry, CreateApiKeyResponse, DashboardStats, LoginResponse, Model,
    Provider, ProviderTypeInfo,
};

use std::cell::{Cell, RefCell};

thread_local! {
    static SUPPRESS_401: Cell<bool> = const { Cell::new(false) };
    static ON_SESSION_EXPIRED: RefCell<Option<Box<dyn Fn() + 'static>>> = const { RefCell::new(None) };
}

pub fn set_session_expired_handler(callback: Box<dyn Fn() + 'static>) {
    ON_SESSION_EXPIRED.with(|cell| {
        *cell.borrow_mut() = Some(callback);
    });
}

fn session_expired() {
    if SUPPRESS_401.get() {
        return;
    }
    ON_SESSION_EXPIRED.with(|cell| {
        if let Some(cb) = cell.borrow().as_ref() {
            (cb)();
        }
    });
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

fn get_base_url() -> String {
    sycamore::web::window()
        .location()
        .origin()
        .unwrap_or_default()
}

// --- Core request helpers ---

async fn response_to_error(resp: gloo_net::http::Response) -> NetError {
    let status = resp.status();
    if status == 401 {
        session_expired();
    }
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
        "api/models",
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

// --- API Key CRUD ---

pub async fn fetch_api_keys(username: &str) -> Result<Vec<ApiKeyListItem>, NetError> {
    api_get(&format!("api/users/{}/api-keys", username)).await
}

pub async fn create_api_key(
    username: &str,
    name: &str,
    expires_at: Option<i64>,
) -> Result<CreateApiKeyResponse, NetError> {
    api_post(
        &format!("api/users/{}/api-keys", username),
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
        &format!("api/users/{}/api-keys/{}", username, key_id),
        &serde_json::json!({ "enabled": enabled }),
    )
    .await
}

pub async fn delete_api_key(username: &str, key: &str) -> Result<(), NetError> {
    api_delete(&format!("api/users/{}/api-keys/{}", username, key)).await
}

// --- Auth ---

pub async fn register_api(
    username: &str,
    password: &str,
    registration_code: &str,
) -> Result<(), NetError> {
    let _guard = Suppress401Guard::new();
    api_post::<serde_json::Value>(
        "auth/register",
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

pub async fn login_api(username: &str, password: &str) -> Result<(), NetError> {
    let _guard = Suppress401Guard::new();
    api_post::<LoginResponse>(
        "auth/login",
        &serde_json::json!({ "username": username, "password": password }),
        &[],
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

pub async fn logout_api() -> Result<(), NetError> {
    let _guard = Suppress401Guard::new();
    api_post::<serde_json::Value>("auth/logout", &serde_json::json!({}), &[]).await?;
    Ok(())
}

// --- Data fetchers ---

pub async fn fetch_providers() -> Result<Vec<Provider>, NetError> {
    api_get("api/providers").await
}

pub async fn fetch_provider_types() -> Result<Vec<ProviderTypeInfo>, NetError> {
    api_get("api/provider-types").await
}

pub async fn fetch_models() -> Result<Vec<Model>, NetError> {
    api_get("api/models").await
}

pub async fn fetch_dashboard_stats() -> Result<DashboardStats, NetError> {
    api_get("api/stats").await
}

pub async fn fetch_request_buckets(
    start_ts: i64,
    end_ts: i64,
) -> Result<Vec<BucketEntry>, NetError> {
    api_get(&format!(
        "api/data/requests?start_ts={}&end_ts={}",
        start_ts, end_ts
    ))
    .await
}

pub async fn fetch_token_buckets(start_ts: i64, end_ts: i64) -> Result<Vec<BucketEntry>, NetError> {
    api_get(&format!(
        "api/data/tokens?start_ts={}&end_ts={}",
        start_ts, end_ts
    ))
    .await
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
        "api/providers",
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
        &format!("api/providers/{}", id),
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
    api_delete(&format!("api/providers/{}", id)).await
}

// --- Text Generation ---

use futures_util::stream::{Stream, try_unfold};
use js_sys::Uint8Array;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

pub async fn generate_completion_stream(
    token: &str,
    model: &str,
    prompt: &str,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    top_p: Option<f32>,
) -> Result<impl Stream<Item = Result<String, String>>, String> {
    let url = format!("{}/v1/completions", get_base_url());
    let mut body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "stream": true,
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

    let opts = web_sys::RequestInit::new();
    opts.set_method("POST");
    opts.set_body(&JsValue::from_str(&body.to_string()));
    let request = web_sys::Request::new_with_str_and_init(&url, &opts)
        .map_err(|e| format!("Failed to create request: {:?}", e))?;
    request
        .headers()
        .set("Content-Type", "application/json")
        .map_err(|_| "Failed to set Content-Type header".to_string())?;
    request
        .headers()
        .set("Authorization", &auth)
        .map_err(|_| "Failed to set Authorization header".to_string())?;

    let window = web_sys::window().ok_or("No window")?;
    let resp_value = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("Fetch failed: {:?}", e))?;
    let resp: web_sys::Response = resp_value
        .dyn_into()
        .map_err(|_| "Fetch did not return a Response".to_string())?;

    if !resp.ok() {
        let status = resp.status();
        let err_text = match resp.text() {
            Ok(p) => JsFuture::from(p)
                .await
                .ok()
                .and_then(|v| v.as_string())
                .unwrap_or_default(),
            Err(_) => String::new(),
        };
        let msg = serde_json::from_str::<serde_json::Value>(&err_text)
            .ok()
            .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(String::from))
            .unwrap_or_else(|| format!("HTTP {}", status));
        return Err(msg);
    }

    let raw_stream = resp.body().ok_or("Response has no body".to_string())?;
    let reader: web_sys::ReadableStreamDefaultReader = raw_stream
        .get_reader()
        .dyn_into()
        .map_err(|_| "Failed to create reader".to_string())?;

    Ok(sse_stream_from_reader(reader))
}

fn sse_stream_from_reader(
    reader: web_sys::ReadableStreamDefaultReader,
) -> impl Stream<Item = Result<String, String>> {
    try_unfold((reader, Vec::<u8>::new()), |(reader, mut buf)| async move {
        loop {
            if let Some(result) = poll_sse(&mut buf) {
                match result {
                    SsePoll::Token(t) => return Ok::<_, String>(Some((t, (reader, buf)))),
                    SsePoll::End => return Ok(None),
                    SsePoll::Skip => continue,
                }
            }

            let promise = reader.read();
            let result = JsFuture::from(promise)
                .await
                .map_err(|e| format!("Stream read error: {:?}", e))?;

            let done = js_sys::Reflect::get(&result, &JsValue::from_str("done"))
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            if done {
                if !buf.is_empty() {
                    let remaining = String::from_utf8_lossy(&buf).to_string();
                    buf.clear();
                    return Ok::<_, String>(Some((remaining, (reader, buf))));
                }
                return Ok(None);
            }

            if let Ok(value) = js_sys::Reflect::get(&result, &JsValue::from_str("value")) {
                let array = Uint8Array::new(&value);
                buf.extend_from_slice(&array.to_vec());
            }
        }
    })
}

enum SsePoll {
    Token(String),
    End,
    Skip,
}

fn poll_sse(buf: &mut Vec<u8>) -> Option<SsePoll> {
    let boundary = buf
        .windows(2)
        .position(|w| w == b"\n\n")
        .or_else(|| buf.windows(4).position(|w| w == b"\r\n\r\n"))?;

    let event_bytes: Vec<u8> = buf.drain(..boundary).collect();
    let n = if buf.len() >= 4
        && buf[0] == b'\r'
        && buf[1] == b'\n'
        && buf[2] == b'\r'
        && buf[3] == b'\n'
    {
        4
    } else if buf.len() >= 2 && buf[0] == b'\n' && buf[1] == b'\n' {
        2
    } else {
        0
    };
    buf.drain(..n);

    let event_str = String::from_utf8_lossy(&event_bytes);
    for line in event_str.lines() {
        let payload = match line.strip_prefix("data: ") {
            Some(p) => p.trim(),
            None => continue,
        };
        if payload == "[DONE]" {
            return Some(SsePoll::End);
        }
        if let Some(token) = parse_token(payload) {
            return Some(SsePoll::Token(token));
        }
    }
    Some(SsePoll::Skip)
}

fn parse_token(payload: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(payload).ok()?;

    if let Some(content) = json
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|c| c.first())
        .and_then(|c| c.get("delta"))
        .and_then(|d| d.get("content"))
        .and_then(|c| c.as_str())
        && !content.is_empty()
    {
        return Some(content.to_string());
    }

    if let Some(text) = json
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|c| c.first())
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        && !text.is_empty()
    {
        return Some(text.to_string());
    }

    if let Some(response) = json.get("response").and_then(|r| r.as_str())
        && !response.is_empty()
    {
        return Some(response.to_string());
    }

    None
}
