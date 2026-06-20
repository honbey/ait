use super::{ProviderCore, UpstreamProvider};
use reqwest::Client;

pub struct OllamaProvider {
    core: ProviderCore,
}

impl OllamaProvider {
    pub fn new(core: ProviderCore) -> Self {
        Self { core }
    }
}

#[async_trait::async_trait]
impl UpstreamProvider for OllamaProvider {
    async fn build_request(
        &self,
        _client: &Client,
        body: &serde_json::Value,
        stream: bool,
        upstream_model: &str,
        upstream_path: &str,
    ) -> Result<reqwest::Request, String> {
        let ollama_path = match upstream_path {
            "/v1/chat/completions" => "/api/chat",
            "/v1/completions" => "/api/generate",
            "/v1/embeddings" => "/api/embed",
            _ => "/api/chat",
        };

        let mut ollama_body = match body.clone() {
            serde_json::Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };

        ollama_body.insert("model".into(), serde_json::json!(upstream_model));
        ollama_body.insert("stream".into(), serde_json::Value::Bool(stream));

        if let Some(max_tokens) = ollama_body.remove("max_tokens") {
            ollama_body.insert("num_predict".into(), max_tokens);
        }

        if let Some(re) = ollama_body.remove("reasoning_effort") {
            let should_disable = matches!(re, serde_json::Value::Null)
                || (re.is_string() && re.as_str() == Some("none"));
            if should_disable {
                ollama_body.insert("think".into(), serde_json::json!(false));
            } else {
                ollama_body.insert("reasoning_effort".into(), re);
            }
        }

        let body_bytes = serde_json::to_vec(&ollama_body).map_err(|e| e.to_string())?;

        let builder = self
            .core
            .client()
            .post(self.core.upstream_url(ollama_path))
            .header("Content-Type", "application/json")
            .body(body_bytes);

        let builder = self.core.apply_auth_header(builder);
        let request = builder.build().map_err(|e| e.to_string())?;
        Ok(request)
    }
}
