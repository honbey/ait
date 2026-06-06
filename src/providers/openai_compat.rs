use super::UpstreamProvider;
use crate::db::Provider;
use reqwest::Client;

pub struct OpenAICompatProvider {
    provider: Provider,
    client: Client,
}

impl OpenAICompatProvider {
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
impl UpstreamProvider for OpenAICompatProvider {
    async fn build_request(
        &self,
        _client: &Client,
        body: &serde_json::Value,
        stream: bool,
        upstream_model: &str,
        upstream_path: &str,
    ) -> Result<reqwest::Request, String> {
        let mut body = body.clone();

        // Replace model name with upstream model name
        if let Some(model_val) = body.get("model") {
            if let Some(_original) = model_val.as_str() {
                body["model"] = serde_json::json!(upstream_model);
            }
        }

        if stream {
            body["stream"] = serde_json::Value::Bool(true);
        }

        let body_bytes = serde_json::to_vec(&body).map_err(|e| e.to_string())?;

        let mut builder = self.client
            .post(self.upstream_url(upstream_path))
            .header("Content-Type", "application/json")
            .body(body_bytes);

        if let Some(ref api_key) = self.provider.api_key {
            builder = builder.header("Authorization", format!("Bearer {}", api_key));
        }

        let request = builder.build().map_err(|e| e.to_string())?;
        Ok(request)
    }
}