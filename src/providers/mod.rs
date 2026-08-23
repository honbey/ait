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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::create_test_provider;

    // ── inject_default_shadow ──

    #[test]
    fn inject_default_shadow_sets_model_and_fingerprint() {
        let mut val = serde_json::json!({"choices": []});
        inject_default_shadow(&mut val, "gpt-4");
        assert_eq!(val["model"], "gpt-4");
        assert_eq!(val["system_fingerprint"], "ait-proxy");
    }

    #[test]
    fn inject_default_shadow_noop_on_non_object() {
        let mut val = serde_json::json!([1, 2, 3]);
        inject_default_shadow(&mut val, "gpt-4");
        assert_eq!(val, serde_json::json!([1, 2, 3]));
    }

    // ── transform_response (default impl) ──

    struct BareProvider;

    #[async_trait::async_trait]
    impl UpstreamProvider for BareProvider {
        async fn build_request(
            &self,
            _client: &Client,
            _body: serde_json::Value,
            _stream: bool,
            _upstream_model: &str,
            _upstream_path: &str,
        ) -> Result<reqwest::Request, String> {
            unreachable!()
        }
    }

    #[test]
    fn transform_response_injects_shadow_on_valid_json() {
        let provider = BareProvider;
        let input = br#"{"choices":[{"message":"hi"}]}"#;
        let output = provider.transform_response(input, "my-model");
        let val: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(val["model"], "my-model");
        assert_eq!(val["system_fingerprint"], "ait-proxy");
    }

    #[test]
    fn transform_response_passes_through_invalid_json() {
        let provider = BareProvider;
        let input = b"not json at all";
        let output = provider.transform_response(input, "my-model");
        assert_eq!(output, input);
    }

    // ── ProviderCore ──

    #[test]
    fn provider_core_upstream_url_strips_trailing_slash() {
        let provider =
            create_test_provider("p1", ProviderType::OpenAICompat, "http://127.0.0.1:8080/");
        let core = ProviderCore::new(&provider, Client::new());
        assert_eq!(
            core.upstream_url("/chat/completions"),
            "http://127.0.0.1:8080/chat/completions"
        );
    }

    #[test]
    fn provider_core_finalize_request_sets_stream_options() {
        let provider =
            create_test_provider("p1", ProviderType::OpenAICompat, "http://127.0.0.1:8080");
        let core = ProviderCore::new(&provider, Client::new());
        let body = serde_json::json!({"model": "gpt-4", "messages": []});
        let request = core
            .finalize_request(body, true, "gpt-4", "/chat/completions")
            .unwrap();
        assert_eq!(request.method(), reqwest::Method::POST);
        assert!(request.url().as_str().ends_with("/chat/completions"));
        let body_bytes = request.body().unwrap().as_bytes().unwrap();
        let body_val: serde_json::Value = serde_json::from_slice(body_bytes).unwrap();
        assert_eq!(body_val["stream"], true);
        assert_eq!(body_val["stream_options"]["include_usage"], true);
        assert_eq!(body_val["model"], "gpt-4");
    }

    #[test]
    fn provider_core_finalize_request_omits_stream_options_for_responses() {
        let provider =
            create_test_provider("p1", ProviderType::OpenAICompat, "http://127.0.0.1:8080");
        let core = ProviderCore::new(&provider, Client::new());
        let body = serde_json::json!({"model": "gpt-4", "input": "hello"});
        let request = core
            .finalize_request(body, true, "gpt-4", "/responses")
            .unwrap();
        let body_bytes = request.body().unwrap().as_bytes().unwrap();
        let body_val: serde_json::Value = serde_json::from_slice(body_bytes).unwrap();
        assert_eq!(body_val["stream"], true);
        assert!(body_val.get("stream_options").is_none());
    }

    #[test]
    fn provider_core_finalize_request_non_stream_no_stream_field() {
        let provider =
            create_test_provider("p1", ProviderType::OpenAICompat, "http://127.0.0.1:8080");
        let core = ProviderCore::new(&provider, Client::new());
        let body = serde_json::json!({"model": "gpt-4", "messages": []});
        let request = core
            .finalize_request(body, false, "gpt-4", "/chat/completions")
            .unwrap();
        let body_bytes = request.body().unwrap().as_bytes().unwrap();
        let body_val: serde_json::Value = serde_json::from_slice(body_bytes).unwrap();
        assert!(body_val.get("stream").is_none());
    }

    #[test]
    fn provider_core_apply_auth_header_with_api_key() {
        let mut provider =
            create_test_provider("p1", ProviderType::OpenAICompat, "http://127.0.0.1:8080");
        provider.api_key = Some("sk-secret".to_string());
        let core = ProviderCore::new(&provider, Client::new());
        let builder = core.client().post("http://127.0.0.1:8080/test");
        let request = core.apply_auth_header(builder).build().unwrap();
        assert_eq!(
            request.headers().get("authorization").unwrap(),
            "Bearer sk-secret"
        );
    }

    #[test]
    fn provider_core_apply_auth_header_without_api_key() {
        let provider =
            create_test_provider("p1", ProviderType::OpenAICompat, "http://127.0.0.1:8080");
        let core = ProviderCore::new(&provider, Client::new());
        let builder = core.client().post("http://127.0.0.1:8080/test");
        let request = core.apply_auth_header(builder).build().unwrap();
        assert!(request.headers().get("authorization").is_none());
    }

    // ── create_provider ──

    #[test]
    fn create_provider_returns_correct_type_for_each_variant() {
        let client = Client::new();
        for provider_type in [
            ProviderType::OpenAICompat,
            ProviderType::DeepSeek,
            ProviderType::Zhipu,
            ProviderType::Ollama,
            ProviderType::Llamacpp,
        ] {
            let provider = create_test_provider("p1", provider_type, "http://127.0.0.1:8080");
            let _upstream = create_provider(&provider, client.clone());
        }
    }
}
