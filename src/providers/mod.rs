pub mod ollama;
pub mod openai_compat;

pub use ollama::OllamaProvider;
pub use openai_compat::OpenAICompatProvider;

use crate::db::{Provider, ProviderType};
use reqwest::Client;

#[async_trait::async_trait]
pub trait UpstreamProvider: Send + Sync {
    /// Build a reqwest Request from the incoming body, ready to send to upstream.
    async fn build_request(
        &self,
        client: &Client,
        body: &serde_json::Value,
        stream: bool,
        upstream_model: &str,
        upstream_path: &str,
    ) -> Result<reqwest::Request, String>;
}

/// Creates the appropriate provider implementation based on provider_type.
pub fn create_provider(provider: &Provider, http_client: Client) -> Box<dyn UpstreamProvider> {
    match provider.provider_type {
        ProviderType::Ollama => Box::new(OllamaProvider::new(provider, http_client)),
        _ => Box::new(OpenAICompatProvider::new(provider, http_client)), // OpenAICompat, DeepSeek, Zhipu, LlamaCpp
    }
}
