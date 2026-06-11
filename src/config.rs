use config::{Config, ConfigError, Environment, File, FileFormat};
use rand::RngExt;
use serde::Deserialize;
use tracing::warn;

#[derive(Debug, Deserialize, Clone)]
pub struct ConfigApp {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub database: DatabaseConfig,
    pub proxy: ProxyConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    /// Whether the health check endpoint returns detailed information
    pub health_detail: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AuthConfig {
    pub enabled: bool,
    pub token: Option<String>,
    /// Admin token for /admin/* endpoints. If not set, falls back to `token`.
    pub admin_token: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub path: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProxyConfig {
    pub timeout_secs: u64,
    pub stream: bool,
}

impl ConfigApp {
    /// Load configuration. `config_path` specifies the config file path (without extension),
    /// defaults to `config/ait` when `None`.
    pub fn new(config_path: Option<&str>) -> Result<Self, ConfigError> {
        let config_file = config_path.unwrap_or("config/ait");

        let mut app: Self = Config::builder()
            .set_default("server.host", "127.0.0.1")?
            .set_default("server.port", 8000u16)?
            .set_default("server.health_detail", false)?
            .set_default("auth.enabled", true)?
            .set_default("auth.token", "")?
            .set_default("auth.admin_token", "")?
            .set_default("database.path", "./data/ait.rocksdb")?
            .set_default("proxy.timeout_secs", 300u64)?
            .set_default("proxy.stream", true)?
            .add_source(File::new(config_file, FileFormat::Toml).required(false))
            .add_source(Environment::with_prefix("AIT").separator("_"))
            .build()?
            .try_deserialize()?;

        // Validate and auto-generate tokens if needed
        let token = app.auth.token.as_deref().unwrap_or("");
        if token.len() < 16 {
            let generated = generate_random_token(32);
            warn!(
                "auth.token is not set or too short (< 16 chars), using auto-generated token: {}",
                generated
            );
            app.auth.token = Some(generated);
        }

        let admin_token = app.auth.admin_token.as_deref().unwrap_or("");
        if admin_token.len() < 16 {
            warn!("auth.admin_token is too short (< 16 chars), using auth.token");
            app.auth.admin_token = app.auth.token.clone();
        }

        Ok(app)
    }
}

/// Generate a random alphanumeric token of the given length.
fn generate_random_token(len: usize) -> String {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::rng();
    (0..len)
        .map(|_| CHARS[rng.random_range(0..CHARS.len())] as char)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ConfigApp::new(None).unwrap();
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 8000);
        assert!(config.auth.enabled);
        assert_eq!(config.database.path, "./data/ait.rocksdb");
    }
}
