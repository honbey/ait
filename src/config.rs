use config::{Config, ConfigError, Environment, File, FileFormat};
use serde::Deserialize;
use std::net::IpAddr;

#[derive(Debug, Deserialize, Clone)]
pub struct ConfigApp {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub database: DatabaseConfig,
    pub log: LogConfig,
    pub proxy: ProxyConfig,
    pub security: SecurityConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub health_detail: bool,
    pub session_cleanup_interval_secs: u64,
    pub rate_limiter_cleanup_interval_secs: u64,
    pub cache_cleanup_interval_secs: u64,
    pub cache_max_entries: u64,
    pub graceful_timeout_secs: u64,
    pub trusted_proxies: Vec<IpAddr>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RateLimitConfig {
    pub max_attempts: u64,
    pub window_secs: u64,
    pub ban_secs: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AuthConfig {
    pub enabled: bool,
    pub session_ttl_secs: u64,
    pub bootstrap_username: String,
    pub bootstrap_password: Option<String>,
    pub max_api_keys_per_user: u64,
    pub rate_limiter_max_entries: u64,
    pub login_rate_limit: RateLimitConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub path: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LogConfig {
    pub path: String,
    pub retention_days: u64,
    pub flush_interval_secs: u64,
    pub flush_batch: u64,
    pub channel_cap: u64,
    pub retention_every: u64,
    pub level: String,
    pub axum: String,
    pub tower_http_trace: String,
    pub analytics_timeout_secs: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProxyConfig {
    pub timeout_secs: u64,
    pub stream: bool,
    pub sse_idle_timeout_secs: u64,
    pub connect_timeout_secs: u64,
    pub max_response_body_bytes: u64,
    pub max_request_body_bytes: u64,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct DlpConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub sensitive_values: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SecurityConfig {
    pub ssrf_allowed_cidrs: Vec<String>,
    pub cors_allowed_origins: Vec<String>,
    pub cors_allow_credentials: bool,
    #[serde(default)]
    pub dlp: DlpConfig,
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
            .set_default("server.session_cleanup_interval_secs", 3600u64)?
            .set_default("server.rate_limiter_cleanup_interval_secs", 600u64)?
            .set_default("server.cache_cleanup_interval_secs", 300u64)?
            .set_default("server.cache_max_entries", 1000u64)?
            .set_default("server.graceful_timeout_secs", 10u64)?
            .set_default("server.trusted_proxies", vec!["127.0.0.1", "::1"])?
            .set_default("auth.enabled", true)?
            .set_default("auth.session_ttl_secs", 86400u64)?
            .set_default("auth.bootstrap_username", "admin")?
            .set_default("auth.max_api_keys_per_user", 10u64)?
            .set_default("auth.rate_limiter_max_entries", 100000u64)?
            .set_default("auth.login_rate_limit.max_attempts", 5u64)?
            .set_default("auth.login_rate_limit.window_secs", 300u64)?
            .set_default("auth.login_rate_limit.ban_secs", 900u64)?
            .set_default("database.path", "./data/ait.db")?
            .set_default("log.path", "./data/ait-logs.duckdb")?
            .set_default("log.retention_days", 30u64)?
            .set_default("log.flush_interval_secs", 10u64)?
            .set_default("log.flush_batch", 100u64)?
            .set_default("log.channel_cap", 10000u64)?
            .set_default("log.retention_every", 100u64)?
            .set_default("log.level", "info")?
            .set_default("log.axum", "info")?
            .set_default("log.tower_http_trace", "info")?
            .set_default("log.analytics_timeout_secs", 10u64)?
            .set_default("proxy.timeout_secs", 300u64)?
            .set_default("proxy.stream", true)?
            .set_default("proxy.sse_idle_timeout_secs", 60u64)?
            .set_default("proxy.connect_timeout_secs", 30u64)?
            .set_default("proxy.max_response_body_bytes", 8u64 * 1024 * 1024)?
            .set_default("proxy.max_request_body_bytes", 8u64 * 1024 * 1024)?
            .set_default("security.ssrf_allowed_cidrs", Vec::<String>::new())?
            .set_default("security.cors_allowed_origins", Vec::<String>::new())?
            .set_default("security.cors_allow_credentials", false)?
            .set_default("security.dlp.enabled", false)?
            .set_default("security.dlp.sensitive_values", Vec::<String>::new())?
            .add_source(File::new(config_file, FileFormat::Toml).required(false))
            .add_source(Environment::with_prefix("AIT").separator("_"))
            .build()?
            .try_deserialize()?;

        // `is_multiple_of(0)` panics and would kill the log worker thread
        if app.log.retention_every == 0 {
            return Err(ConfigError::Message(
                "log.retention_every must be >= 1 (0 crashes the log worker)".to_string(),
            ));
        }

        Ok(app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_toml(content: &str) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_config.toml");
        std::fs::write(&path, content).unwrap();
        let stem = dir.path().join("test_config");
        (dir, stem.to_str().unwrap().to_string())
    }

    #[test]
    fn test_default_config() {
        let config = ConfigApp::new(Some("config/ait.toml.example")).unwrap();
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 8000);
        assert_eq!(config.server.session_cleanup_interval_secs, 3600);
        assert_eq!(config.server.rate_limiter_cleanup_interval_secs, 600);
        assert!(config.auth.enabled);
        assert_eq!(config.auth.max_api_keys_per_user, 10);
        assert_eq!(config.database.path, "./data/ait.db");
        assert_eq!(config.log.path, "./data/ait-logs.duckdb");
        assert_eq!(config.log.retention_days, 30);
        assert_eq!(config.log.flush_interval_secs, 10);
        assert_eq!(config.log.flush_batch, 100);
        assert_eq!(config.log.channel_cap, 10000);
        assert_eq!(config.log.retention_every, 100);
        assert_eq!(config.log.level, "info");
        assert_eq!(config.log.axum, "info");
        assert_eq!(config.log.tower_http_trace, "info");
        assert_eq!(config.log.analytics_timeout_secs, 10);
        assert_eq!(config.auth.login_rate_limit.max_attempts, 5);
        assert_eq!(config.auth.login_rate_limit.window_secs, 300);
        assert_eq!(config.auth.login_rate_limit.ban_secs, 900);
    }

    #[test]
    fn config_toml_override() {
        let (_dir, path) = write_toml(
            r#"
[server]
port = 9090
health_detail = true

[auth]
bootstrap_username = "custom_admin"
max_api_keys_per_user = 20
"#,
        );
        let config = ConfigApp::new(Some(&path)).unwrap();

        // overridden
        assert_eq!(config.server.port, 9090);
        assert!(config.server.health_detail);
        assert_eq!(config.auth.bootstrap_username, "custom_admin");
        assert_eq!(config.auth.max_api_keys_per_user, 20);

        // still defaults
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.session_cleanup_interval_secs, 3600);
        assert_eq!(config.database.path, "./data/ait.db");
        assert_eq!(config.proxy.timeout_secs, 300);
        assert!(config.proxy.stream);
        assert_eq!(config.proxy.max_response_body_bytes, 8 * 1024 * 1024);
    }

    #[test]
    fn test_missing_file() {
        let config = ConfigApp::new(Some("/nonexistent/path/that/does/not/exist")).unwrap();
        assert_eq!(config.server.port, 8000);
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.auth.max_api_keys_per_user, 10);
        assert_eq!(config.log.retention_days, 30);
        assert_eq!(config.log.analytics_timeout_secs, 10);
        assert_eq!(config.proxy.max_response_body_bytes, 8 * 1024 * 1024);
        assert!(config.security.ssrf_allowed_cidrs.is_empty());
        assert!(config.security.cors_allowed_origins.is_empty());
        assert!(!config.security.cors_allow_credentials);
    }

    #[test]
    fn cors_allow_credentials_override() {
        let (_dir, path) = write_toml(
            r#"
[security]
cors_allow_credentials = true
cors_allowed_origins = ["https://app.example.com"]
"#,
        );
        let config = ConfigApp::new(Some(&path)).unwrap();
        assert!(config.security.cors_allow_credentials);
        assert_eq!(
            config.security.cors_allowed_origins,
            vec!["https://app.example.com"]
        );
    }

    #[test]
    fn retention_every_zero_rejected() {
        let (_dir, path) = write_toml(
            r#"
[log]
retention_every = 0
"#,
        );
        let err = ConfigApp::new(Some(&path)).unwrap_err();
        assert!(err.to_string().contains("retention_every"));
    }

    #[test]
    fn invalid_toml_rejected() {
        let (_dir, path) = write_toml("this is not [ valid toml");
        assert!(ConfigApp::new(Some(&path)).is_err());
    }

    #[test]
    fn proxy_and_security_toml_override() {
        let (_dir, path) = write_toml(
            r#"
[proxy]
timeout_secs = 60
stream = false
max_response_body_bytes = 1048576

[security]
ssrf_allowed_cidrs = ["10.0.0.0/8"]
cors_allowed_origins = ["https://app.example.com"]
cors_allow_credentials = true
"#,
        );
        let config = ConfigApp::new(Some(&path)).unwrap();
        assert_eq!(config.proxy.timeout_secs, 60);
        assert!(!config.proxy.stream);
        assert_eq!(config.proxy.max_response_body_bytes, 1048576);
        assert_eq!(config.security.ssrf_allowed_cidrs, vec!["10.0.0.0/8"]);
        assert_eq!(
            config.security.cors_allowed_origins,
            vec!["https://app.example.com"]
        );
        assert!(config.security.cors_allow_credentials);
    }

    #[test]
    fn dlp_defaults_disabled() {
        let config = ConfigApp::new(Some("config/ait.toml.example")).unwrap();
        assert!(!config.security.dlp.enabled);
        assert!(config.security.dlp.sensitive_values.is_empty());
        assert_eq!(config.proxy.max_request_body_bytes, 8 * 1024 * 1024);
    }

    #[test]
    fn dlp_toml_override() {
        let (_dir, path) = write_toml(
            r#"
[proxy]
max_request_body_bytes = 1048576

[security.dlp]
enabled = true
sensitive_values = ["110101199001011234", "13800138000"]
"#,
        );
        let config = ConfigApp::new(Some(&path)).unwrap();
        assert!(config.security.dlp.enabled);
        assert_eq!(
            config.security.dlp.sensitive_values,
            vec!["110101199001011234", "13800138000"]
        );
        assert_eq!(config.proxy.max_request_body_bytes, 1048576);
    }
}
