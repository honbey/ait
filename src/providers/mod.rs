pub mod llamacpp;
pub mod ollama;
pub mod openai_compat;

pub use llamacpp::LlamacppProvider;
pub use ollama::OllamaProvider;
pub use openai_compat::OpenAICompatProvider;

use crate::db::{Provider, ProviderType};
use reqwest::Client;
use std::sync::Arc;

pub fn inject_default_shadow(val: &mut serde_json::Value, model_name: &str) {
    if let Some(obj) = val.as_object_mut() {
        obj.insert("model".to_string(), serde_json::json!(model_name));
        obj.insert(
            "system_fingerprint".to_string(),
            serde_json::json!("ait-proxy"),
        );
    }
}

#[async_trait::async_trait]
pub trait UpstreamProvider: Send + Sync {
    async fn build_request(
        &self,
        client: &Client,
        body: serde_json::Value,
        stream: bool,
        upstream_model: &str,
        upstream_path: &str,
    ) -> Result<reqwest::Request, String>;

    fn transform_response(&self, body: &[u8], model_name: &str) -> Vec<u8> {
        let Ok(mut val) = serde_json::from_slice::<serde_json::Value>(body) else {
            return body.to_vec();
        };
        inject_default_shadow(&mut val, model_name);
        serde_json::to_vec(&val).unwrap_or_else(|_| body.to_vec())
    }
}

pub struct ProviderCore {
    provider: Provider,
    auth_header_value: Option<String>,
    client: Client,
}

impl ProviderCore {
    pub fn new(provider: &Provider, client: Client) -> Self {
        let auth_header_value = provider.api_key.as_ref().map(|k| format!("Bearer {}", k));
        Self {
            provider: provider.clone(),
            auth_header_value,
            client,
        }
    }

    pub fn upstream_url(&self, path: &str) -> String {
        let base = self.provider.base_url.trim_end_matches('/');
        format!("{}{}", base, path)
    }

    pub fn apply_auth_header(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(ref value) = self.auth_header_value {
            builder.header("Authorization", value.as_str())
        } else {
            builder
        }
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub fn finalize_request(
        &self,
        mut body: serde_json::Value,
        stream: bool,
        upstream_model: &str,
        upstream_path: &str,
    ) -> Result<reqwest::Request, String> {
        if body.get("model").and_then(|m| m.as_str()).is_some() {
            body["model"] = serde_json::json!(upstream_model);
        }

        if stream {
            body["stream"] = serde_json::Value::Bool(true);
            // stream_options is a chat.completions-only parameter; the
            // Responses API reports usage in its terminal SSE event.
            if upstream_path != "/responses" {
                body["stream_options"] = serde_json::json!({"include_usage": true});
            }
        }

        let body_bytes = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
        self.apply_auth_header(
            self.client()
                .post(self.upstream_url(upstream_path))
                .header("Content-Type", "application/json")
                .body(body_bytes),
        )
        .build()
        .map_err(|e| e.to_string())
    }
}

pub fn create_provider(provider: &Provider, http_client: Client) -> Arc<dyn UpstreamProvider> {
    let core = ProviderCore::new(provider, http_client);
    match provider.provider_type {
        ProviderType::Ollama => Arc::new(OllamaProvider::new(core)),
        ProviderType::Llamacpp => Arc::new(LlamacppProvider::new(core)),
        _ => Arc::new(OpenAICompatProvider::new(core)),
    }
}
