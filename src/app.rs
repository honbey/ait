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
pub(crate) type ModelCacheEntry = (Option<(Model, Provider)>, Instant);
type ProviderCacheEntry = (Arc<dyn UpstreamProvider>, Instant);

/// Negative-cache entries (unknown models) expire fast and never slide,
/// so spraying bogus model names cannot grow the model_cache unbounded.
pub(crate) const NEGATIVE_CACHE_TTL: Duration = Duration::from_secs(30);

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
            .connect_timeout(std::time::Duration::from_secs(
                config.proxy.connect_timeout_secs,
            ))
            .tcp_keepalive(Some(std::time::Duration::from_secs(60)))
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
/// Note: `max_entries` is only a soft threshold here; the model cache also
/// enforces it as a hard cap at insertion time (see insert_model_cache).
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
                        cleanup_caches(&caches.0, &caches.1, &caches.2, &caches.3, &caches.4, max_entries)
                    }).await {
                        tracing::warn!("Cache cleanup error: {e}");
                    }
                }
            }
        }
    });
}

/// Evict expired in-memory cache entries. Extracted from the cleanup task so
/// tests can exercise the retain patterns without a running background task.
/// Note: `max_entries` is only a soft threshold here; the model cache also
/// enforces it as a hard cap at insertion time (see insert_model_cache).
#[allow(clippy::too_many_arguments)]
pub(crate) fn cleanup_caches(
    session_cache: &DashMap<String, SessionCacheEntry>,
    api_key_cache: &DashMap<String, (ApiKeyInfo, Instant)>,
    model_cache: &DashMap<String, ModelCacheEntry>,
    provider_cache: &DashMap<String, ProviderCacheEntry>,
    ssrf_dns_cache: &DashMap<String, (Vec<IpAddr>, Instant)>,
    max_entries: usize,
) {
    let threshold = max_entries.saturating_mul(9) / 10;
    let aggressive = |len: usize| {
        if len > threshold {
            CACHE_TTL / 2
        } else {
            CACHE_TTL
        }
    };
    let ttl = aggressive(session_cache.len());
    session_cache.retain(|_, v| v.2.elapsed() < ttl);
    let ttl = aggressive(api_key_cache.len());
    api_key_cache.retain(|_, v| v.1.elapsed() < ttl);
    let ttl = aggressive(model_cache.len());
    // compute before retain: calling len() inside the retain
    // closure would take a read lock while holding the write
    // lock and deadlock (parking_lot is not reentrant)
    model_cache.retain(|_, v| {
        let entry_ttl = if v.0.is_none() {
            NEGATIVE_CACHE_TTL
        } else {
            ttl
        };
        v.1.elapsed() < entry_ttl
    });
    let ttl = aggressive(provider_cache.len());
    provider_cache.retain(|_, v| v.1.elapsed() < ttl);
    let ttl = aggressive(ssrf_dns_cache.len());
    ssrf_dns_cache.retain(|_, v| v.1.elapsed() < ttl);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppInitError;
    use crate::test_utils::test_config;
    use tempfile::TempDir;

    fn temp_config(with_bootstrap: bool) -> (ConfigApp, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let log_path = dir.path().join("logs.duckdb");
        let mut config = test_config(db_path.to_str().unwrap(), log_path.to_str().unwrap());
        if with_bootstrap {
            config.auth.bootstrap_password = Some("admin123".to_string());
        }
        (config, dir)
    }

    fn bootstrap_config(with_password: bool) -> (crate::config::AuthConfig, TempDir) {
        let (config, dir) = temp_config(false);
        let mut auth = config.auth;
        if with_password {
            auth.bootstrap_password = Some("admin123".to_string());
        }
        (auth, dir)
    }

    #[test]
    fn bootstrap_empty_db_without_password_errors() {
        let (auth, _dir) = bootstrap_config(false);
        let (db, _dir2) = crate::test_utils::create_test_db(10);
        let err = bootstrap_user_if_needed(&db, &auth).unwrap_err();
        assert!(matches!(err, AppInitError::BootstrapUser(_)));
    }

    #[test]
    fn bootstrap_empty_db_with_password_creates_user() {
        let (auth, _dir) = bootstrap_config(true);
        let (db, _dir2) = crate::test_utils::create_test_db(10);
        bootstrap_user_if_needed(&db, &auth).unwrap();
        let user = db.get_user(&auth.bootstrap_username).unwrap().unwrap();
        assert_eq!(user.username, auth.bootstrap_username);
        assert!(bcrypt::verify("admin123", &user.password_hash).unwrap());
    }

    #[test]
    fn bootstrap_existing_user_skips_creation() {
        let (auth, _dir) = bootstrap_config(true);
        let (db, _dir2) = crate::test_utils::create_test_db(10);
        crate::test_utils::insert_test_user(&db, "existing", "pw");
        bootstrap_user_if_needed(&db, &auth).unwrap();
        // The configured bootstrap user was not created.
        assert!(db.get_user(&auth.bootstrap_username).unwrap().is_none());
    }

    #[tokio::test]
    async fn app_state_new_success_with_bootstrap() {
        let (config, _dir) = temp_config(true);
        let state = AppState::new(config).unwrap();
        assert_eq!(state.config.auth.bootstrap_username, "admin");
        // The bootstrap user exists in the state's database.
        let username = state.config.auth.bootstrap_username.clone();
        let db = state.db.clone();
        assert!(
            crate::run_blocking(move || db.get_user(&username))
                .await
                .unwrap()
                .unwrap()
                .is_some()
        );
        state.shutdown_token.cancel();
        state.log_manager.shutdown();
    }

    #[tokio::test]
    async fn app_state_new_missing_bootstrap_password_fails() {
        let (config, _dir) = temp_config(false);
        let err = match AppState::new(config) {
            Ok(_) => panic!("AppState::new should fail without bootstrap password"),
            Err(e) => e,
        };
        assert!(matches!(err, AppInitError::BootstrapUser(_)));
    }

    // ── DashMap deadlock prevention tests ──
    //
    // parking_lot RwLock is not reentrant: holding a Ref/RefMut and calling
    // insert/retain/remove on the same map deadlocks, including sneaky cases
    // where match scrutinee temporaries keep the guard alive. These tests
    // exercise the production access patterns and detect regressions via
    // assert_no_deadlock (separate thread + recv_timeout).

    use crate::test_utils::{assert_no_deadlock, create_test_model, create_test_provider};

    struct MockProvider;

    #[async_trait::async_trait]
    impl UpstreamProvider for MockProvider {
        async fn build_request(
            &self,
            _client: &reqwest::Client,
            _body: serde_json::Value,
            _stream: bool,
            _upstream_model: &str,
            _upstream_path: &str,
        ) -> Result<reqwest::Request, String> {
            Err("mock provider is not usable".to_string())
        }
    }

    fn test_session_user() -> SessionUser {
        SessionUser {
            username: "alice".to_string(),
            api_key_name: None,
        }
    }

    fn test_api_key_info() -> ApiKeyInfo {
        ApiKeyInfo {
            id: "key-1".to_string(),
            username: "alice".to_string(),
            name: "test-key".to_string(),
            enabled: true,
            expires_at: None,
            created_at: chrono::Utc::now(),
        }
    }

    fn new_session_cache() -> Arc<DashMap<String, SessionCacheEntry>> {
        Arc::new(DashMap::new())
    }

    fn new_api_key_cache() -> Arc<DashMap<String, (ApiKeyInfo, Instant)>> {
        Arc::new(DashMap::new())
    }

    fn new_model_cache() -> Arc<DashMap<String, ModelCacheEntry>> {
        Arc::new(DashMap::new())
    }

    fn new_provider_cache() -> Arc<DashMap<String, ProviderCacheEntry>> {
        Arc::new(DashMap::new())
    }

    fn new_ssrf_cache() -> Arc<DashMap<String, (Vec<IpAddr>, Instant)>> {
        Arc::new(DashMap::new())
    }

    fn stale() -> Instant {
        Instant::now() - Duration::from_secs(3600)
    }

    fn fresh() -> Instant {
        Instant::now()
    }

    #[test]
    fn middleware_cache_pattern_get_mut_then_drop_then_mutate() {
        // Mirrors auth_middleware/admin_auth_middleware: read-modify the
        // entry, drop the guard, then mutate the same map.
        let session_cache = new_session_cache();
        let api_key_cache = new_api_key_cache();
        session_cache.insert("s1".to_string(), (test_session_user(), Utc::now(), fresh()));
        api_key_cache.insert("k1".to_string(), (test_api_key_info(), fresh()));

        assert_no_deadlock(Duration::from_secs(5), {
            let session_cache = session_cache.clone();
            let api_key_cache = api_key_cache.clone();
            move || {
                if let Some(mut entry) = session_cache.get_mut("s1") {
                    entry.2 = Instant::now();
                }
                session_cache.remove("s1");
                session_cache.insert("s2".to_string(), (test_session_user(), Utc::now(), fresh()));

                if let Some(mut entry) = api_key_cache.get_mut("k1") {
                    entry.1 = Instant::now();
                }
                api_key_cache.remove("k1");
                api_key_cache.insert("k2".to_string(), (test_api_key_info(), fresh()));
            }
        });

        assert!(session_cache.contains_key("s2"));
        assert!(!session_cache.contains_key("s1"));
        assert!(api_key_cache.contains_key("k2"));
        assert!(!api_key_cache.contains_key("k1"));
    }

    #[test]
    fn handler_cache_patterns_insert_remove_retain() {
        // Mirrors handlers/providers.rs update/delete: provider_cache.remove
        // and model_cache.retain on a populated map.
        let model_cache = new_model_cache();
        let provider_cache = new_provider_cache();
        let ssrf_cache = new_ssrf_cache();
        let provider = create_test_provider(
            "p1",
            crate::db::ProviderType::OpenAICompat,
            "http://127.0.0.1:8080",
        );
        let model = create_test_model("m1", "p1");
        model_cache.insert("m1".to_string(), (Some((model, provider)), fresh()));
        provider_cache.insert("p1".to_string(), (Arc::new(MockProvider), fresh()));
        ssrf_cache.insert(
            "host1".to_string(),
            (vec!["1.2.3.4".parse().unwrap()], fresh()),
        );

        assert_no_deadlock(Duration::from_secs(5), {
            let model_cache = model_cache.clone();
            let provider_cache = provider_cache.clone();
            let ssrf_cache = ssrf_cache.clone();
            move || {
                provider_cache.remove("p1");
                model_cache.retain(|_, v| v.0.as_ref().is_none_or(|(_, p)| p.id != "p1"));
                ssrf_cache.retain(|_, v| v.1.elapsed() < Duration::from_secs(300));
            }
        });

        assert!(model_cache.is_empty());
        assert!(provider_cache.is_empty());
        assert!(!ssrf_cache.is_empty());
    }

    #[test]
    fn cleanup_caches_evicts_expired_keeps_fresh() {
        let session_cache = new_session_cache();
        let api_key_cache = new_api_key_cache();
        let model_cache = new_model_cache();
        let provider_cache = new_provider_cache();
        let ssrf_cache = new_ssrf_cache();

        session_cache.insert(
            "s-old".to_string(),
            (test_session_user(), Utc::now(), stale()),
        );
        session_cache.insert(
            "s-new".to_string(),
            (test_session_user(), Utc::now(), fresh()),
        );
        api_key_cache.insert("k-old".to_string(), (test_api_key_info(), stale()));
        api_key_cache.insert("k-new".to_string(), (test_api_key_info(), fresh()));
        // Negative model cache entry (unknown model) and a positive one.
        model_cache.insert("neg".to_string(), (None, stale()));
        model_cache.insert(
            "pos-old".to_string(),
            (
                Some((
                    create_test_model("m", "p"),
                    create_test_provider(
                        "p",
                        crate::db::ProviderType::OpenAICompat,
                        "http://127.0.0.1:8080",
                    ),
                )),
                stale(),
            ),
        );
        model_cache.insert(
            "pos-new".to_string(),
            (
                Some((
                    create_test_model("m2", "p"),
                    create_test_provider(
                        "p",
                        crate::db::ProviderType::OpenAICompat,
                        "http://127.0.0.1:8080",
                    ),
                )),
                fresh(),
            ),
        );
        provider_cache.insert("p-old".to_string(), (Arc::new(MockProvider), stale()));
        provider_cache.insert("p-new".to_string(), (Arc::new(MockProvider), fresh()));
        ssrf_cache.insert(
            "d-old".to_string(),
            (vec!["1.2.3.4".parse().unwrap()], stale()),
        );
        ssrf_cache.insert(
            "d-new".to_string(),
            (vec!["1.2.3.4".parse().unwrap()], fresh()),
        );

        assert_no_deadlock(Duration::from_secs(5), {
            let session_cache = session_cache.clone();
            let api_key_cache = api_key_cache.clone();
            let model_cache = model_cache.clone();
            let provider_cache = provider_cache.clone();
            let ssrf_cache = ssrf_cache.clone();
            move || {
                cleanup_caches(
                    &session_cache,
                    &api_key_cache,
                    &model_cache,
                    &provider_cache,
                    &ssrf_cache,
                    1000,
                );
            }
        });

        assert_eq!(session_cache.len(), 1);
        assert!(session_cache.contains_key("s-new"));
        assert_eq!(api_key_cache.len(), 1);
        assert!(api_key_cache.contains_key("k-new"));
        // Negative entries use NEGATIVE_CACHE_TTL; both old entries gone.
        assert_eq!(model_cache.len(), 1);
        assert!(model_cache.contains_key("pos-new"));
        assert_eq!(provider_cache.len(), 1);
        assert!(provider_cache.contains_key("p-new"));
        assert_eq!(ssrf_cache.len(), 1);
        assert!(ssrf_cache.contains_key("d-new"));
    }

    #[test]
    fn cleanup_caches_uses_aggressive_ttl_over_threshold() {
        let session_cache = new_session_cache();
        let api_key_cache = new_api_key_cache();
        let model_cache = new_model_cache();
        let provider_cache = new_provider_cache();
        let ssrf_cache = new_ssrf_cache();

        // 200s-old entries survive the normal 300s TTL but not the aggressive
        // 150s TTL triggered when len() > 90% of max_entries.
        let mid_old = Instant::now() - Duration::from_secs(200);
        for i in 0..10 {
            session_cache.insert(format!("s{i}"), (test_session_user(), Utc::now(), mid_old));
        }
        api_key_cache.insert("k0".to_string(), (test_api_key_info(), mid_old));
        model_cache.insert("m0".to_string(), (None, mid_old));
        provider_cache.insert("p0".to_string(), (Arc::new(MockProvider), mid_old));
        ssrf_cache.insert(
            "d0".to_string(),
            (vec!["1.2.3.4".parse().unwrap()], mid_old),
        );

        assert_no_deadlock(Duration::from_secs(5), {
            let session_cache = session_cache.clone();
            let api_key_cache = api_key_cache.clone();
            let model_cache = model_cache.clone();
            let provider_cache = provider_cache.clone();
            let ssrf_cache = ssrf_cache.clone();
            move || {
                cleanup_caches(
                    &session_cache,
                    &api_key_cache,
                    &model_cache,
                    &provider_cache,
                    &ssrf_cache,
                    10,
                );
            }
        });

        // len was 10 > threshold 9, so session_cache used CACHE_TTL/2 = 150s
        // and dropped the 200s-old entries. The other caches hold a single
        // entry each and keep the normal 300s TTL, except the negative model
        // cache entry which always expires after NEGATIVE_CACHE_TTL (30s).
        assert!(session_cache.is_empty());
        assert_eq!(api_key_cache.len(), 1);
        assert!(model_cache.is_empty());
        assert_eq!(provider_cache.len(), 1);
        assert_eq!(ssrf_cache.len(), 1);
    }

    #[test]
    fn cleanup_caches_with_concurrent_writers_no_deadlock() {
        let session_cache = new_session_cache();
        let api_key_cache = new_api_key_cache();
        let model_cache = new_model_cache();
        let provider_cache = new_provider_cache();
        let ssrf_cache = new_ssrf_cache();
        // Seed entries so the cleanup task has something to walk.
        for i in 0..50 {
            session_cache.insert(format!("s{i}"), (test_session_user(), Utc::now(), fresh()));
        }
        for i in 0..50 {
            api_key_cache.insert(format!("k{i}"), (test_api_key_info(), fresh()));
        }

        let cleanup_cache = session_cache.clone();
        let cleanup_api = api_key_cache.clone();
        let cleanup_model = model_cache.clone();
        let cleanup_provider = provider_cache.clone();
        let cleanup_ssrf = ssrf_cache.clone();

        assert_no_deadlock(Duration::from_secs(10), move || {
            let mut handles = Vec::new();
            // One thread running the periodic cleanup (retain on all caches).
            handles.push(std::thread::spawn(move || {
                for _ in 0..100 {
                    cleanup_caches(
                        &cleanup_cache,
                        &cleanup_api,
                        &cleanup_model,
                        &cleanup_provider,
                        &cleanup_ssrf,
                        1000,
                    );
                }
            }));
            // Four writers inserting/removing concurrently with the cleanup.
            for _ in 0..4 {
                let session_cache = session_cache.clone();
                let api_key_cache = api_key_cache.clone();
                let model_cache = model_cache.clone();
                let provider_cache = provider_cache.clone();
                let ssrf_cache = ssrf_cache.clone();
                handles.push(std::thread::spawn(move || {
                    for i in 0..200 {
                        let key = format!("w{i}");
                        session_cache
                            .insert(key.clone(), (test_session_user(), Utc::now(), fresh()));
                        api_key_cache.insert(key.clone(), (test_api_key_info(), fresh()));
                        model_cache.insert(key.clone(), (None, fresh()));
                        provider_cache.insert(key.clone(), (Arc::new(MockProvider), fresh()));
                        ssrf_cache.insert(key.clone(), (vec!["1.2.3.4".parse().unwrap()], fresh()));
                        session_cache.remove(&key);
                        api_key_cache.remove(&key);
                    }
                }));
            }
            for handle in handles {
                handle.join().unwrap();
            }
        });
    }

    #[test]
    fn concurrent_mixed_operations_no_deadlock() {
        // 4 threads hammering a shared cache with the full mix of operations
        // used across the codebase: get, get_mut (guard dropped), insert,
        // remove, retain.
        let cache: Arc<DashMap<String, (i64, Instant)>> = Arc::new(DashMap::new());
        for i in 0..20 {
            cache.insert(format!("k{i}"), (i, fresh()));
        }

        let mut handles = Vec::new();
        for t in 0..4 {
            let cache = cache.clone();
            handles.push(std::thread::spawn(move || {
                for i in 0..500 {
                    let key = format!("k{}", (i + t) % 24);
                    let _ = cache.get(&key);
                    if let Some(mut entry) = cache.get_mut(&key) {
                        entry.1 = Instant::now();
                    }
                    cache.insert(format!("t{t}-{i}"), (i, fresh()));
                    if i % 3 == 0 {
                        cache.remove(&key);
                    }
                    if i % 7 == 0 {
                        cache.retain(|_, v| v.1.elapsed() < Duration::from_secs(300));
                    }
                }
            }));
        }
        assert_no_deadlock(Duration::from_secs(10), move || {
            for handle in handles {
                handle.join().unwrap();
            }
        });
    }

    #[test]
    fn positive_control_detects_held_guard_deadlock() {
        // Deliberately reproduce the banned pattern (insert while a RefMut is
        // still alive) to prove assert_no_deadlock can detect a deadlock.
        // The stuck thread leaks until process exit; that is the point.
        let cache: Arc<DashMap<String, i64>> = Arc::new(DashMap::new());
        cache.insert("k".to_string(), 0);
        let cache_clone = cache.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            assert_no_deadlock(Duration::from_millis(500), move || {
                let mut entry = cache_clone.get_mut("k").unwrap();
                *entry = 1;
                // Deadlock: insert on the same key while the RefMut guard is
                // alive re-enters the same shard's write lock.
                cache_clone.insert("k".to_string(), 2);
            });
        }));
        assert!(
            result.is_err(),
            "assert_no_deadlock must report the held-guard deadlock"
        );
    }
}
