use std::net::IpAddr;

use crate::config::ConfigApp;
use crate::db::Database;
use crate::db::logger::LogManager;
use crate::db::{ApiKeyInfo, Model, Provider, SessionUser};
use crate::error::AppInitError;
use crate::handlers::users::create_user;
use crate::middleware::CACHE_TTL;
use crate::providers::UpstreamProvider;
use crate::rate_limiter::RateLimiter;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

type SessionCacheEntry = (SessionUser, DateTime<Utc>, Instant);
type ModelCacheEntry = (Option<(Model, Provider)>, Instant);
type ProviderCacheEntry = (Arc<dyn UpstreamProvider>, Instant);

#[derive(Clone)]
pub struct AppState {
    pub config: ConfigApp,
    pub db: Arc<Database>,
    pub http_client: reqwest::Client,
    pub log_manager: LogManager,
    pub start_time: DateTime<Utc>,
    pub login_rate_limiter: RateLimiter,
    pub shutdown_token: CancellationToken,
    pub api_key_cache: Arc<DashMap<String, (ApiKeyInfo, Instant)>>,
    pub session_cache: Arc<DashMap<String, SessionCacheEntry>>,
    pub model_cache: Arc<DashMap<String, ModelCacheEntry>>,
    pub provider_cache: Arc<DashMap<String, ProviderCacheEntry>>,
    pub ssrf_dns_cache: Arc<DashMap<String, (Vec<IpAddr>, Instant)>>,
}

impl AppState {
    pub fn new(config: ConfigApp) -> Result<Self, AppInitError> {
        if !config.auth.enabled {
            tracing::warn!(
                "Authentication is disabled — proxy requests will have full access. \
                 Set [auth].enabled = true in config to enable authentication."
            );
        }

        // Created up front so every background task can subscribe to it; the
        // same instance ends up in AppState at the end.
        let shutdown_token = CancellationToken::new();

        let db = Arc::new(
            Database::new(&config.database.path, config.auth.max_api_keys_per_user)
                .map_err(AppInitError::Database)?,
        );

        let http_client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(config.proxy.timeout_secs))
            .build()
            .map_err(AppInitError::HttpClient)?;

        bootstrap_user_if_needed(&db, &config.auth)?;

        // All fallible initialisation must complete before the first
        // `tokio::spawn` — if anything above returns `Err`, the caller
        // will exit immediately without orphaned background tasks.
        let log_manager = LogManager::new(&config.log).map_err(AppInitError::LogManager)?;

        // Rate limiters are built before spawning their cleanup tasks so they
        // can also live on the returned AppState.
        let max_entries = config.auth.rate_limiter_max_entries as usize;
        let login_limiter = RateLimiter::new(max_entries);
        let cleanup_interval = config.server.rate_limiter_cleanup_interval_secs;
        let login_rl = config.auth.login_rate_limit.clone();

        spawn_rate_limiter_cleanup(
            login_limiter.clone(),
            shutdown_token.clone(),
            cleanup_interval,
            login_rl.window_secs,
        );

        spawn_session_cleanup(
            db.clone(),
            shutdown_token.clone(),
            config.server.session_cleanup_interval_secs,
        );

        let api_key_cache: Arc<DashMap<String, (ApiKeyInfo, Instant)>> = Arc::new(DashMap::new());
        let session_cache: Arc<DashMap<String, SessionCacheEntry>> = Arc::new(DashMap::new());
        let model_cache: Arc<DashMap<String, ModelCacheEntry>> = Arc::new(DashMap::new());
        let provider_cache: Arc<DashMap<String, ProviderCacheEntry>> = Arc::new(DashMap::new());
        let ssrf_dns_cache: Arc<DashMap<String, (Vec<IpAddr>, Instant)>> = Arc::new(DashMap::new());
        spawn_caches_cleanup(
            session_cache.clone(),
            api_key_cache.clone(),
            model_cache.clone(),
            provider_cache.clone(),
            ssrf_dns_cache.clone(),
            shutdown_token.clone(),
            config.server.cache_cleanup_interval_secs,
            config.server.cache_max_entries as usize,
        );

        Ok(Self {
            config,
            db,
            http_client,
            log_manager,
            start_time: Utc::now(),
            login_rate_limiter: login_limiter,
            shutdown_token,
            api_key_cache,
            session_cache,
            model_cache,
            provider_cache,
            ssrf_dns_cache,
        })
    }
}

/// Create the initial user when the database has none.
///
/// Returns `Err` when bootstrap credentials are missing — caller
/// (currently `AppState::new`) is responsible for logging.
fn bootstrap_user_if_needed(
    db: &Database,
    auth: &crate::config::AuthConfig,
) -> Result<(), AppInitError> {
    if db.has_any_users().unwrap_or(false) {
        return Ok(());
    }
    let password = auth.bootstrap_password.as_deref().ok_or_else(|| {
        AppInitError::BootstrapUser("bootstrap_password not set and no user exists".to_string())
    })?;
    let user =
        create_user(db, &auth.bootstrap_username, password).map_err(AppInitError::BootstrapUser)?;
    tracing::info!("Created initial user '{}'", user.username);
    Ok(())
}

/// Periodic cleanup of expired sessions, stops when `shutdown` fires.
fn spawn_session_cleanup(db: Arc<Database>, shutdown: CancellationToken, interval_secs: u64) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = interval.tick() => {
                    let db = db.clone();
                    match crate::run_blocking(move || db.cleanup_expired_sessions()).await {
                        Ok(Ok(count)) => {
                            if count > 0 {
                                tracing::info!("Cleaned up {} expired sessions", count);
                            }
                        }
                        Ok(Err(e)) => tracing::error!("Session cleanup error: {}", e),
                        Err(join_err) => tracing::error!("Session cleanup task failed: {}", join_err),
                    }
                }
            }
        }
    });
}

/// Evict expired in-memory cache entries at a configurable interval.
/// Applies a shorter TTL when a cache exceeds 90% of max_entries.
/// Covers session, api_key, model, provider, and SSRF DNS caches in one task.
#[allow(clippy::too_many_arguments)]
fn spawn_caches_cleanup(
    session_cache: Arc<DashMap<String, SessionCacheEntry>>,
    api_key_cache: Arc<DashMap<String, (ApiKeyInfo, Instant)>>,
    model_cache: Arc<DashMap<String, ModelCacheEntry>>,
    provider_cache: Arc<DashMap<String, ProviderCacheEntry>>,
    ssrf_dns_cache: Arc<DashMap<String, (Vec<IpAddr>, Instant)>>,
    shutdown: CancellationToken,
    interval_secs: u64,
    max_entries: usize,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = interval.tick() => {
                    let caches = (
                        session_cache.clone(),
                        api_key_cache.clone(),
                        model_cache.clone(),
                        provider_cache.clone(),
                        ssrf_dns_cache.clone(),
                    );
                    if let Err(e) = crate::run_blocking(move || {
                        let threshold = max_entries.saturating_mul(9) / 10;
                        let aggressive = |len: usize| if len > threshold { CACHE_TTL / 2 } else { CACHE_TTL };
                        let ttl = aggressive(caches.0.len());
                        caches.0.retain(|_, v| v.2.elapsed() < ttl);
                        let ttl = aggressive(caches.1.len());
                        caches.1.retain(|_, v| v.1.elapsed() < ttl);
                        let ttl = aggressive(caches.2.len());
                        caches.2.retain(|_, v| v.1.elapsed() < ttl);
                        let ttl = aggressive(caches.3.len());
                        caches.3.retain(|_, v| v.1.elapsed() < ttl);
                        let ttl = aggressive(caches.4.len());
                        caches.4.retain(|_, v| v.1.elapsed() < ttl);
                    }).await {
                        tracing::warn!("Cache cleanup error: {e}");
                    }
                }
            }
        }
    });
}

fn spawn_rate_limiter_cleanup(
    limiter: RateLimiter,
    shutdown: CancellationToken,
    interval_secs: u64,
    window_secs: u64,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = interval.tick() => {
                    let limiter = limiter.clone();
                    if let Err(e) = crate::run_blocking(move || {
                        limiter.cleanup(window_secs);
                    }).await {
                        tracing::warn!("Rate limiter cleanup error: {e}");
                    }
                }
            }
        }
    });
}
