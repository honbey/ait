// Developed against Ollama 0.30.10
use super::{ProviderCore, UpstreamProvider, inject_default_shadow};
use reqwest::Client;

pub struct OllamaProvider {
    core: ProviderCore,
}

impl OllamaProvider {
    pub fn new(core: ProviderCore) -> Self {
        Self { core }
    }
}

fn rename_reasoning_key(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(map) => {
            if let Some(val) = map.remove("reasoning") {
                map.insert("reasoning_content".to_string(), val);
            }
            for (_key, val) in map.iter_mut() {
                rename_reasoning_key(val);
            }
        }
        serde_json::Value::Array(arr) => {
            for val in arr.iter_mut() {
                rename_reasoning_key(val);
            }
        }
        _ => {}
    }
}

#[async_trait::async_trait]
impl UpstreamProvider for OllamaProvider {
    fn transform_response(&self, body: &[u8], model_name: &str) -> Vec<u8> {
        let Ok(mut val) = serde_json::from_slice::<serde_json::Value>(body) else {
            return body.to_vec();
        };
        inject_default_shadow(&mut val, model_name);
        rename_reasoning_key(&mut val);
        serde_json::to_vec(&val).unwrap_or_else(|_| body.to_vec())
    }

    async fn build_request(
        &self,
        _client: &Client,
        body: &serde_json::Value,
        stream: bool,
        upstream_model: &str,
        upstream_path: &str,
    ) -> Result<reqwest::Request, String> {
        self.core
            .finalize_request(body.clone(), stream, upstream_model, upstream_path)
    }
}
