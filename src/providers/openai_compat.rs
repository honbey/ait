use super::{ProviderCore, UpstreamProvider};
use reqwest::Client;

pub struct OpenAICompatProvider {
    core: ProviderCore,
}

impl OpenAICompatProvider {
    pub fn new(core: ProviderCore) -> Self {
        Self { core }
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

        if body.get("model").and_then(|m| m.as_str()).is_some() {
            body["model"] = serde_json::json!(upstream_model);
        }

        if stream {
            body["stream"] = serde_json::Value::Bool(true);
            body["stream_options"] = serde_json::json!({"include_usage": true});
        }

        let body_bytes = serde_json::to_vec(&body).map_err(|e| e.to_string())?;

        let builder = self
            .core
            .client()
            .post(self.core.upstream_url(upstream_path))
            .header("Content-Type", "application/json")
            .body(body_bytes);

        let builder = self.core.apply_auth_header(builder);
        let request = builder.build().map_err(|e| e.to_string())?;
        Ok(request)
    }
}
