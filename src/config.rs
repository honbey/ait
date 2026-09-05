use config::{Config, ConfigError, Environment, File, FileFormat};
use serde::Deserialize;
use std::collections::HashMap;
use std::net::IpAddr;

/// A trusted reverse proxy: an exact address or a CIDR block.
///
/// A bare address is not enough in practice: an ingress on another host, or
/// one rescheduled inside a subnet, cannot be listed without a CIDR or a
/// config change on every move.
#[derive(Debug, Clone)]
pub enum TrustedProxy {
    Ip(IpAddr),
    Cidr(IpAddr, u8),
}

impl TrustedProxy {
    /// Parse `addr` or `addr/prefix`.
    ///
    /// Used by [`Config`] deserialization, so an unusable entry fails at
    /// startup instead of silently never matching anything.
    pub(crate) fn parse(entry: &str) -> Result<Self, String> {
        let Some((addr, prefix)) = entry.split_once('/') else {
            return entry
                .parse()
                .map(TrustedProxy::Ip)
                .map_err(|_| format!("'{entry}' is not an IP address or CIDR block"));
        };
        let addr: IpAddr = addr
            .parse()
            .map_err(|_| format!("'{entry}' is not an IP address or CIDR block"))?;
        // The bound follows the entry's own address family, matching
        // `ssrf::ip_in_cidr`; a wider default would over-match.
        let family_max = match addr {
            IpAddr::V4(_) => 32u8,
            IpAddr::V6(_) => 128u8,
        };
        let len: u8 = prefix
            .parse()
            .map_err(|_| format!("'{entry}' has a prefix that is not a number"))?;
        if len > family_max {
            return Err(format!("'{entry}' prefix must be <= {family_max}"));
        }
        Ok(TrustedProxy::Cidr(addr, len))
    }

    /// Whether `ip` comes from this proxy.
    pub fn contains(&self, ip: IpAddr) -> bool {
        let (prefix, len) = match *self {
            TrustedProxy::Ip(addr) => (addr, if addr.is_ipv4() { 32 } else { 128 }),
            TrustedProxy::Cidr(addr, len) => (addr, len),
        };
        // Canonicalize both sides, so an IPv4-mapped peer matches an IPv4
        // entry; a full-length prefix is then an exact match.
        crate::ssrf::ip_in_prefix(&ip, &prefix, len)
    }
}

impl<'de> Deserialize<'de> for TrustedProxy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let entry = String::deserialize(deserializer)?;
        TrustedProxy::parse(&entry).map_err(serde::de::Error::custom)
    }
}

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
    pub cache_cleanup_interval_secs: u64,
    pub cache_max_entries: u64,
    pub graceful_timeout_secs: u64,
    /// Reverse proxies allowed to set `X-Forwarded-For` / `Remote-User`.
    /// Each entry is an IP address or a CIDR block; an unusable entry is
    /// rejected at startup. Empty means no header is trusted.
    pub trusted_proxies: Vec<TrustedProxy>,
    /// Number of trusted reverse proxies in front of Ait. X-Forwarded-For is
    /// read `hops` entries from the right: the nearest trusted proxy appends
    /// the peer it saw, so leftmost entries are client-controlled and can be
    /// freely spoofed. `0` ignores the header entirely.
    pub trusted_proxy_hops: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AuthConfig {
    pub enabled: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub path: String,
    /// Read-only connections opened alongside the single writer. WAL lets them
    /// serve reads concurrently with the writer and with each other, so this
    /// caps how many lookups run at once - including the two on the proxy hot
    /// path, model resolution and API key verification. Each connection keeps
    /// its own page cache, a few MB.
    pub reader_pool_size: usize,
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
    /// Workers draining the analytics queue. Each owns a DuckDB connection, so
    /// this caps concurrent analytics queries; a request that finds them all
    /// busy waits and fails with 503 after `analytics_timeout_secs`.
    pub analytics_workers: usize,
    /// Memory ceiling for the log/analytics database, in MiB. DuckDB otherwise
    /// sizes itself from the memory it can see, which inside a container is the
    /// host rather than the cgroup limit - enough to get the process
    /// OOM-killed part way through a large scan.
    pub duckdb_memory_limit_mb: u64,
    /// DuckDB threads per query.
    pub duckdb_threads: u64,
    #[serde(default)]
    pub loki: LokiConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LokiConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub url: String,
    #[serde(default = "default_loki_labels")]
    pub labels: HashMap<String, String>,
    #[serde(default = "default_loki_batch")]
    pub batch_size: u64,
    #[serde(default = "default_loki_interval")]
    pub interval_secs: u64,
    #[serde(default = "default_loki_timeout")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub basic_auth_user: Option<String>,
    #[serde(default)]
    pub basic_auth_password: Option<String>,
    #[serde(default)]
    pub bearer_token: Option<String>,
}

impl Default for LokiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: String::new(),
            labels: default_loki_labels(),
            batch_size: default_loki_batch(),
            interval_secs: default_loki_interval(),
            timeout_secs: default_loki_timeout(),
            basic_auth_user: None,
            basic_auth_password: None,
            bearer_token: None,
        }
    }
}

fn default_loki_labels() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("app".to_string(), "ait".to_string());
    m
}
fn default_loki_batch() -> u64 {
    100
}
fn default_loki_interval() -> u64 {
    5
}
fn default_loki_timeout() -> u64 {
    5
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProxyConfig {
    pub timeout_secs: u64,
    pub stream: bool,
    pub sse_idle_timeout_secs: u64,
    /// Hard cap on the total lifetime of a streaming response. Without it an
    /// upstream that trickles a byte before each idle deadline keeps the
    /// connection open indefinitely.
    pub sse_max_duration_secs: u64,
    pub connect_timeout_secs: u64,
    pub max_response_body_bytes: u64,
    pub max_request_body_bytes: u64,
    /// Divisor for the rough prompt-token estimate used until the upstream
    /// reports usage: tokens ~= request body bytes / divisor. Larger values
    /// estimate fewer tokens. Must be 1-5; 0 would panic on division.
    pub prompt_token_divisor: u64,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct DlpConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub sensitive_values: Vec<String>,
    /// Scan JSON number literals as well, not only string values. Only a
    /// purely numeric rule can ever appear as a number, so this is off by
    /// default: enabling it costs an extra allocation per number leaf.
    #[serde(default)]
    pub scan_numbers: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SecurityConfig {
    pub ssrf_allowed_cidrs: Vec<String>,
    pub cors_allowed_origins: Vec<String>,
    pub cors_allow_credentials: bool,
    #[serde(default)]
    pub dlp: DlpConfig,
}

/// Upper bound for `log.retention_days`: 100 years. The value is cast to i64
/// in two places (the analytics range bound and the cleanup cutoff), and
/// anything larger would wrap or panic inside `Duration::days`.
const MAX_RETENTION_DAYS: u64 = 36_500;

/// Reject a duration whose zero form means "no wait" at every consumer:
/// `Duration::from_secs(0)` is accepted everywhere but makes
/// `tokio::time::interval` panic, the log worker's `recv_timeout` spin on an
/// empty channel, and a proxied request fail the moment it is issued.
fn require_positive_secs(secs: u64, key: &str) -> Result<(), ConfigError> {
    if secs == 0 {
        return Err(ConfigError::Message(format!("{key} must be >= 1")));
    }
    Ok(())
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
            .set_default("server.cache_cleanup_interval_secs", 300u64)?
            .set_default("server.cache_max_entries", 1000u64)?
            .set_default("server.graceful_timeout_secs", 10u64)?
            .set_default("server.trusted_proxies", vec!["127.0.0.1", "::1"])?
            .set_default("server.trusted_proxy_hops", 1u64)?
            .set_default("auth.enabled", true)?
            .set_default("database.path", "./data/ait.db")?
            .set_default("database.reader_pool_size", 4u64)?
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
            .set_default("log.analytics_workers", 2u64)?
            .set_default("log.duckdb_memory_limit_mb", 512u64)?
            .set_default("log.duckdb_threads", 2u64)?
            .set_default("proxy.timeout_secs", 300u64)?
            .set_default("proxy.stream", true)?
            .set_default("proxy.sse_idle_timeout_secs", 60u64)?
            .set_default("proxy.sse_max_duration_secs", 1800u64)?
            .set_default("proxy.connect_timeout_secs", 30u64)?
            .set_default("proxy.max_response_body_bytes", 8u64 * 1024 * 1024)?
            .set_default("proxy.max_request_body_bytes", 8u64 * 1024 * 1024)?
            .set_default("proxy.prompt_token_divisor", 3u64)?
            .set_default("security.ssrf_allowed_cidrs", Vec::<String>::new())?
            .set_default("security.cors_allowed_origins", Vec::<String>::new())?
            .set_default("security.cors_allow_credentials", false)?
            .set_default("security.dlp.enabled", false)?
            .set_default("security.dlp.sensitive_values", Vec::<String>::new())?
            .set_default("security.dlp.scan_numbers", false)?
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

        // Division by zero in the prompt-token estimate would panic in the
        // proxy hot path; outside 1..=5 the estimate is meaningless anyway.
        if !(1..=5).contains(&app.proxy.prompt_token_divisor) {
            return Err(ConfigError::Message(
                "proxy.prompt_token_divisor must be between 1 and 5".to_string(),
            ));
        }

        // Every consumer treats 0 as "do not wait", which is never the intent:
        // the cache cleanup task panics on `interval(0)`, the log worker spins
        // on `recv_timeout(0)`, and a zero HTTP timeout fails each request as
        // soon as it is applied.
        for (secs, key) in [
            (
                app.server.cache_cleanup_interval_secs,
                "server.cache_cleanup_interval_secs",
            ),
            (app.log.flush_interval_secs, "log.flush_interval_secs"),
            (app.proxy.timeout_secs, "proxy.timeout_secs"),
            (
                app.proxy.sse_idle_timeout_secs,
                "proxy.sse_idle_timeout_secs",
            ),
            (app.proxy.connect_timeout_secs, "proxy.connect_timeout_secs"),
        ] {
            require_positive_secs(secs, key)?;
        }

        // A zero-capacity sync channel is a rendezvous channel: every event is
        // dropped whenever the worker is busy flushing.
        if app.log.channel_cap == 0 {
            return Err(ConfigError::Message(
                "log.channel_cap must be >= 1".to_string(),
            ));
        }

        // Bounded rather than merely positive. A pool sized from a request
        // would hand out a SQLite connection per caller and defeat the point
        // of pooling; unbounded DuckDB workers would let concurrent scans
        // fight over the memory ceiling configured below.
        if !(1..=32).contains(&app.database.reader_pool_size) {
            return Err(ConfigError::Message(
                "database.reader_pool_size must be between 1 and 32".to_string(),
            ));
        }
        if !(1..=8).contains(&app.log.analytics_workers) {
            return Err(ConfigError::Message(
                "log.analytics_workers must be between 1 and 8".to_string(),
            ));
        }
        if app.log.duckdb_memory_limit_mb == 0 {
            return Err(ConfigError::Message(
                "log.duckdb_memory_limit_mb must be >= 1".to_string(),
            ));
        }
        require_positive_secs(app.log.duckdb_threads, "log.duckdb_threads")?;

        if app.log.retention_days > MAX_RETENTION_DAYS {
            return Err(ConfigError::Message(format!(
                "log.retention_days must be <= {MAX_RETENTION_DAYS}"
            )));
        }

        // A zero timeout makes every analytics query fail on arrival.
        if app.log.analytics_timeout_secs == 0 {
            return Err(ConfigError::Message(
                "log.analytics_timeout_secs must be >= 1".to_string(),
            ));
        }

        // Mirroring arbitrary origins while allowing credentials lets any site
        // read authenticated responses, so refuse to start instead of silently
        // downgrading to a permissive CORS policy.
        if app.security.cors_allow_credentials
            && app.security.cors_allowed_origins.iter().any(|o| o == "*")
        {
            return Err(ConfigError::Message(
                "security.cors_allowed_origins = [\"*\"] cannot be combined with \
                 security.cors_allow_credentials = true: it reflects any Origin \
                 while allowing credentials. List explicit origins instead."
                    .to_string(),
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
        assert_eq!(config.server.trusted_proxy_hops, 1);
        assert!(config.auth.enabled);
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
        assert_eq!(config.proxy.prompt_token_divisor, 3);
    }

    #[test]
    fn config_toml_override() {
        let (_dir, path) = write_toml(
            r#"
[server]
port = 9090
health_detail = true
"#,
        );
        let config = ConfigApp::new(Some(&path)).unwrap();

        // overridden
        assert_eq!(config.server.port, 9090);
        assert!(config.server.health_detail);

        // still defaults
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.database.path, "./data/ait.db");
        assert_eq!(config.proxy.timeout_secs, 300);
        assert!(config.proxy.stream);
        assert_eq!(config.proxy.max_response_body_bytes, 8 * 1024 * 1024);
    }

    #[test]
    fn wildcard_cors_origin_with_credentials_is_rejected() {
        let (_dir, path) = write_toml(
            r#"
[security]
cors_allowed_origins = ["*"]
cors_allow_credentials = true
"#,
        );
        let err = ConfigApp::new(Some(&path)).unwrap_err();
        assert!(
            err.to_string().contains("cors_allow_credentials"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn wildcard_cors_origin_without_credentials_is_accepted() {
        let (_dir, path) = write_toml(
            r#"
[security]
cors_allowed_origins = ["*"]
"#,
        );
        let config = ConfigApp::new(Some(&path)).unwrap();
        assert_eq!(config.security.cors_allowed_origins, vec!["*".to_string()]);
        assert!(!config.security.cors_allow_credentials);
    }

    #[test]
    fn sse_max_duration_secs_has_default() {
        let config = ConfigApp::new(Some("config/ait.toml.example")).unwrap();
        assert_eq!(config.proxy.sse_max_duration_secs, 1800);
    }

    #[test]
    fn test_missing_file() {
        let config = ConfigApp::new(Some("/nonexistent/path/that/does/not/exist")).unwrap();
        assert_eq!(config.server.port, 8000);
        assert_eq!(config.server.host, "127.0.0.1");
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
    fn retention_days_beyond_bound_rejected() {
        let (_dir, path) = write_toml(
            r#"
[log]
retention_days = 99999999999999999
"#,
        );
        let err = ConfigApp::new(Some(&path)).unwrap_err();
        assert!(
            err.to_string().contains("retention_days"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn analytics_timeout_zero_rejected() {
        let (_dir, path) = write_toml(
            r#"
[log]
analytics_timeout_secs = 0
"#,
        );
        let err = ConfigApp::new(Some(&path)).unwrap_err();
        assert!(
            err.to_string().contains("analytics_timeout_secs"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn prompt_token_divisor_out_of_range_rejected() {
        for bad in [0, 6] {
            let (_dir, path) = write_toml(&format!(
                r#"
[proxy]
prompt_token_divisor = {bad}
"#
            ));
            let err = ConfigApp::new(Some(&path)).unwrap_err();
            assert!(
                err.to_string().contains("prompt_token_divisor"),
                "unexpected error: {err}"
            );
        }
    }

    #[test]
    fn zero_durations_and_channel_cap_rejected() {
        // Each of these silently breaks a loop or the proxy path: interval(0)
        // panics, recv_timeout(0) spins, a zero HTTP timeout fails every
        // request on arrival, and channel_cap 0 drops every log event.
        for (section, key) in [
            ("server", "cache_cleanup_interval_secs"),
            ("log", "flush_interval_secs"),
            ("log", "channel_cap"),
            ("proxy", "timeout_secs"),
            ("proxy", "sse_idle_timeout_secs"),
            ("proxy", "connect_timeout_secs"),
        ] {
            let (_dir, path) = write_toml(&format!("[{section}]\n{key} = 0\n"));
            let err = ConfigApp::new(Some(&path)).unwrap_err();
            assert!(err.to_string().contains(key), "unexpected error: {err}");
        }
    }

    #[test]
    fn trusted_proxy_hops_override() {
        let (_dir, path) = write_toml(
            r#"
[server]
trusted_proxy_hops = 3
"#,
        );
        let config = ConfigApp::new(Some(&path)).unwrap();
        assert_eq!(config.server.trusted_proxy_hops, 3);
    }

    #[test]
    fn trusted_proxy_entries_accept_ip_and_cidr() {
        let (_dir, path) = write_toml(
            r#"
[server]
trusted_proxies = ["127.0.0.1", "::1", "10.0.0.0/8", "2001:db8::/32"]
"#,
        );
        let config = ConfigApp::new(Some(&path)).unwrap();
        let trusted = &config.server.trusted_proxies;
        assert_eq!(trusted.len(), 4);

        // Exact addresses stay exact.
        assert!(trusted[0].contains("127.0.0.1".parse().unwrap()));
        assert!(!trusted[0].contains("127.0.0.2".parse().unwrap()));
        // CIDR blocks cover their range and nothing outside it.
        assert!(trusted[2].contains("10.9.9.9".parse().unwrap()));
        assert!(!trusted[2].contains("11.0.0.1".parse().unwrap()));
        assert!(trusted[3].contains("2001:db8::1".parse().unwrap()));
        assert!(!trusted[3].contains("2001:db9::1".parse().unwrap()));
    }

    #[test]
    fn trusted_proxy_matches_ipv4_mapped_peer() {
        let (_dir, path) = write_toml(
            r#"
[server]
trusted_proxies = ["127.0.0.1"]
"#,
        );
        let config = ConfigApp::new(Some(&path)).unwrap();
        // A peer seen over IPv6 as an IPv4-mapped address is the same host.
        assert!(config.server.trusted_proxies[0].contains("::ffff:127.0.0.1".parse().unwrap()));
    }

    #[test]
    fn invalid_trusted_proxy_entry_rejected() {
        for bad in ["not-an-ip", "10.0.0.0/33", "10.0.0.0/abc", "10.0.0.0/"] {
            let (_dir, path) = write_toml(&format!(
                r#"
[server]
trusted_proxies = ["{bad}"]
"#
            ));
            assert!(
                ConfigApp::new(Some(&path)).is_err(),
                "'{bad}' must be rejected"
            );
        }
    }

    #[test]
    fn pool_and_worker_defaults() {
        let config = ConfigApp::new(Some("config/ait.toml.example")).unwrap();
        assert_eq!(config.database.reader_pool_size, 4);
        assert_eq!(config.log.analytics_workers, 2);
        assert_eq!(config.log.duckdb_memory_limit_mb, 512);
        assert_eq!(config.log.duckdb_threads, 2);
    }

    #[test]
    fn pool_and_worker_overrides() {
        let (_dir, path) = write_toml(
            r#"
[database]
reader_pool_size = 8

[log]
analytics_workers = 4
duckdb_memory_limit_mb = 1024
duckdb_threads = 4
"#,
        );
        let config = ConfigApp::new(Some(&path)).unwrap();
        assert_eq!(config.database.reader_pool_size, 8);
        assert_eq!(config.log.analytics_workers, 4);
        assert_eq!(config.log.duckdb_memory_limit_mb, 1024);
        assert_eq!(config.log.duckdb_threads, 4);
    }

    #[test]
    fn pool_and_worker_bounds_rejected() {
        // Each of these either starves the pool or lets concurrent scans fight
        // over the memory ceiling instead of finishing sooner.
        for (section, key, value) in [
            ("database", "reader_pool_size", "0"),
            ("database", "reader_pool_size", "33"),
            ("log", "analytics_workers", "0"),
            ("log", "analytics_workers", "9"),
            ("log", "duckdb_memory_limit_mb", "0"),
            ("log", "duckdb_threads", "0"),
        ] {
            let (_dir, path) = write_toml(&format!("[{section}]\n{key} = {value}\n"));
            assert!(
                ConfigApp::new(Some(&path)).is_err(),
                "{key} = {value} must be rejected"
            );
        }
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
        assert!(!config.security.dlp.scan_numbers);
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

    #[test]
    fn dlp_scan_numbers_override() {
        let (_dir, path) = write_toml(
            r#"
[security.dlp]
enabled = true
scan_numbers = true
sensitive_values = ["13800138000"]
"#,
        );
        let config = ConfigApp::new(Some(&path)).unwrap();
        assert!(config.security.dlp.enabled);
        assert!(config.security.dlp.scan_numbers);
    }
}
