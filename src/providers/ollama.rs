use super::UpstreamProvider;
use crate::db::Provider;
use reqwest::Client;

pub struct OllamaProvider {
    provider: Provider,
    client: Client,
}

impl OllamaProvider {
    pub fn new(provider: &Provider, client: Client) -> Self {
        Self {
            provider: provider.clone(),
            client,
        }
    }

    fn upstream_url(&self, path: &str) -> String {
        let base = self.provider.base_url.trim_end_matches('/');
        format!("{}{}", base, path)
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
        // Map OpenAI paths to Ollama endpoints
        let ollama_path = match upstream_path {
            "/v1/chat/completions" => "/api/chat",
            "/v1/completions" => "/api/generate",
            "/v1/embeddings" => "/api/embed",
            _ => "/api/chat",
        };
        // Start with a clone of the entire body to forward all fields as-is
        let mut ollama_body = match body.clone() {
            serde_json::Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };

        // Override model with upstream model name
        ollama_body.insert("model".into(), serde_json::json!(upstream_model));

        // Override stream
        ollama_body.insert("stream".into(), serde_json::Value::Bool(stream));

        // Map max_tokens -> num_predict (Ollama uses a different field name)
        if let Some(max_tokens) = ollama_body.remove("max_tokens") {
            ollama_body.insert("num_predict".into(), max_tokens);
        }

        // Handle reasoning_effort: if null or "none", convert to think: false
        if let Some(re) = ollama_body.remove("reasoning_effort") {
            let should_disable = matches!(re, serde_json::Value::Null)
                || (re.is_string() && re.as_str() == Some("none"));
            if should_disable {
                ollama_body.insert("think".into(), serde_json::json!(false));
            }
            // Otherwise keep the original reasoning_effort field
            else {
                ollama_body.insert("reasoning_effort".into(), re);
            }
        }

        let body_bytes = serde_json::to_vec(&ollama_body).map_err(|e| e.to_string())?;

        let mut builder = self.client
            .post(self.upstream_url(ollama_path))
            .header("Content-Type", "application/json")
            .body(body_bytes);

        // Ollama typically doesn't need API key auth
        if let Some(ref api_key) = self.provider.api_key {
            builder = builder.header("Authorization", format!("Bearer {}", api_key));
        }

        let request = builder.build().map_err(|e| e.to_string())?;
        Ok(request)
    }
}