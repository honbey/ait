use config::{Config, ConfigError, Environment, File, FileFormat};
use serde::Deserialize;

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
    /// Session TTL in seconds for web login sessions.
    #[serde(default = "default_session_ttl")]
    pub session_ttl_secs: u64,
    /// Whether to bootstrap an admin user on first startup.
    #[serde(default)]
    pub bootstrap_admin: bool,
    /// Username for the bootstrapped admin user.
    #[serde(default = "default_bootstrap_username")]
    pub bootstrap_username: String,
    /// Password for the bootstrapped admin user.
    #[serde(default = "default_bootstrap_password")]
    pub bootstrap_password: String,
}

fn default_session_ttl() -> u64 {
    86400
}
fn default_bootstrap_username() -> String {
    "admin".to_string()
}
fn default_bootstrap_password() -> String {
    "admin123".to_string()
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

        let app: Self = Config::builder()
            .set_default("server.host", "127.0.0.1")?
            .set_default("server.port", 8000u16)?
            .set_default("server.health_detail", false)?
            .set_default("auth.enabled", true)?
            .set_default("auth.session_ttl_secs", 86400u64)?
            .set_default("auth.bootstrap_admin", false)?
            .set_default("auth.bootstrap_username", "admin")?
            .set_default("auth.bootstrap_password", "admin123")?
            .set_default("database.path", "./data/ait.rocksdb")?
            .set_default("proxy.timeout_secs", 300u64)?
            .set_default("proxy.stream", true)?
            .add_source(File::new(config_file, FileFormat::Toml).required(false))
            .add_source(Environment::with_prefix("AIT").separator("_"))
            .build()?
            .try_deserialize()?;

        Ok(app)
    }
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
