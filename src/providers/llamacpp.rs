// Developed against llama.cpp b9823
use super::{ProviderCore, UpstreamProvider, inject_default_shadow};
use reqwest::Client;

pub struct LlamacppProvider {
    core: ProviderCore,
}

impl LlamacppProvider {
    pub fn new(core: ProviderCore) -> Self {
        Self { core }
    }
}

fn remove_timings(v: &mut serde_json::Value) {
    if let Some(obj) = v.as_object_mut() {
        obj.remove("timings");
    }
}

#[async_trait::async_trait]
impl UpstreamProvider for LlamacppProvider {
    fn transform_response(&self, body: &[u8], model_name: &str) -> Vec<u8> {
        let Ok(mut val) = serde_json::from_slice::<serde_json::Value>(body) else {
            return body.to_vec();
        };
        inject_default_shadow(&mut val, model_name);
        remove_timings(&mut val);
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
        let mut body = body.clone();

        match body.get("reasoning_effort").and_then(|v| v.as_str()) {
            Some("none") => {
                body.as_object_mut().map(|m| m.remove("reasoning_effort"));
                body["chat_template_kwargs"] = serde_json::json!({
                    "enable_thinking": false
                });
            }
            Some("low") | Some("medium") | Some("high") | Some("max") => {
                body.as_object_mut().map(|m| m.remove("reasoning_effort"));
                body["chat_template_kwargs"] = serde_json::json!({
                    "enable_thinking": true
                });
            }
            _ => {}
        }

        self.core
            .finalize_request(body, stream, upstream_model, upstream_path)
    }
}
