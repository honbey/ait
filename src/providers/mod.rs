pub mod ollama;
pub mod openai_compat;

pub use ollama::OllamaProvider;
pub use openai_compat::OpenAICompatProvider;

use crate::db::{Provider, ProviderType};
use reqwest::Client;

#[async_trait::async_trait]
pub trait UpstreamProvider: Send + Sync {
    async fn build_request(
        &self,
        client: &Client,
        body: &serde_json::Value,
        stream: bool,
        upstream_model: &str,
        upstream_path: &str,
    ) -> Result<reqwest::Request, String>;
}

pub struct ProviderCore {
    provider: Provider,
    client: Client,
}

impl ProviderCore {
    pub fn new(provider: &Provider, client: Client) -> Self {
        Self {
            provider: provider.clone(),
            client,
        }
    }

    pub fn upstream_url(&self, path: &str) -> String {
        let base = self.provider.base_url.trim_end_matches('/');
        format!("{}{}", base, path)
    }

    pub fn apply_auth_header(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(ref api_key) = self.provider.api_key {
            builder.header("Authorization", format!("Bearer {}", api_key))
        } else {
            builder
        }
    }

    pub fn client(&self) -> &Client {
        &self.client
    }
}

pub fn create_provider(provider: &Provider, http_client: Client) -> Box<dyn UpstreamProvider> {
    let core = ProviderCore::new(provider, http_client);
    match provider.provider_type {
        ProviderType::Ollama => Box::new(OllamaProvider::new(core)),
        _ => Box::new(OpenAICompatProvider::new(core)),
    }
}
