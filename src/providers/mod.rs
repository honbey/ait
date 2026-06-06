pub mod openai_compat;
pub mod ollama;

pub use openai_compat::OpenAICompatProvider;
pub use ollama::OllamaProvider;

use crate::db::{Provider, ProviderType};
use reqwest::Client;
use serde::Serialize;

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

// --- Shared OpenAI-compatible types (minimal subset for proxying) ---

#[derive(Debug, Serialize)]
pub struct OpenAIError {
    pub message: String,
    pub code: u16,
    pub r#type: String,
}

impl OpenAIError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            code: 400,
            r#type: "invalid_request_error".to_string(),
        }
    }

    pub fn unauthorized() -> Self {
        Self {
            message: "Unauthorized: invalid or missing API key".to_string(),
            code: 401,
            r#type: "auth_error".to_string(),
        }
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            code: 404,
            r#type: "not_found_error".to_string(),
        }
    }

    pub fn upstream_error(status: u16, msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            code: status,
            r#type: "upstream_error".to_string(),
        }
    }

    pub fn internal_error(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            code: 500,
            r#type: "internal_error".to_string(),
        }
    }
}