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
        body: serde_json::Value,
        stream: bool,
        upstream_model: &str,
        upstream_path: &str,
    ) -> Result<reqwest::Request, String> {
        self.core
            .finalize_request(body, stream, upstream_model, upstream_path)
    }
}
