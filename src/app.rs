use std::net::IpAddr;

use crate::config::ConfigApp;
use crate::db::Database;
use crate::db::logger::LogManager;
use crate::db::{ApiKeyInfo, Model, Provider};
use crate::dlp::DlpScanner;
use crate::error::AppInitError;
use crate::middleware::CACHE_TTL;
use crate::providers::UpstreamProvider;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

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
    pub shutdown_token: CancellationToken,
    pub api_key_cache: Arc<DashMap<String, (Option<ApiKeyInfo>, Instant)>>,
    pub model_cache: Arc<DashMap<String, ModelCacheEntry>>,
    pub provider_cache: Arc<DashMap<String, ProviderCacheEntry>>,
    pub ssrf_dns_cache: Arc<DashMap<String, (Vec<IpAddr>, Instant)>>,
    /// Per `host:port` clients whose DNS is pinned to SSRF-verified IPs
    /// (see `ssrf::pinned_client`). Bounded by the configured provider hosts.
    pub pinned_clients: Arc<DashMap<String, (reqwest::Client, Instant)>>,
    pub dlp: DlpScanner,
}

impl AppState {
    pub fn new(config: ConfigApp) -> Result<Self, AppInitError> {
        if !config.auth.enabled {
            tracing::warn!(
                "Authentication is disabled — proxy requests will have full access. \
                 Set [auth].enabled = true in config to enable authentication."
            );
        }
        tracing::info!(
            "Admin API (/api/*) is not authenticated by Ait — \
             deploy a reverse proxy (e.g. nginx + Authelia) to protect it."
        );

        // Created up front so every background task can subscribe to it; the
        // same instance ends up in AppState at the end.
        let shutdown_token = CancellationToken::new();

        let db = Arc::new(Database::new(&config.database.path).map_err(AppInitError::Database)?);

        let http_client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_secs(
                config.proxy.connect_timeout_secs,
            ))
            .tcp_keepalive(Some(std::time::Duration::from_secs(60)))
            .build()
            .map_err(AppInitError::HttpClient)?;

        // All fallible initialisation must complete before the first
        // `tokio::spawn` — if anything above returns `Err`, the caller
        // will exit immediately without orphaned background tasks.
        let log_manager = LogManager::new(&config.log).map_err(AppInitError::LogManager)?;

        let api_key_cache: Arc<DashMap<String, (Option<ApiKeyInfo>, Instant)>> =
            Arc::new(DashMap::new());
        let model_cache: Arc<DashMap<String, ModelCacheEntry>> = Arc::new(DashMap::new());
        let provider_cache: Arc<DashMap<String, ProviderCacheEntry>> = Arc::new(DashMap::new());
        let ssrf_dns_cache: Arc<DashMap<String, (Vec<IpAddr>, Instant)>> = Arc::new(DashMap::new());
        let pinned_clients: Arc<DashMap<String, (reqwest::Client, Instant)>> =
            Arc::new(DashMap::new());
        let dlp = DlpScanner::new(&config.security.dlp);
        spawn_caches_cleanup(
            api_key_cache.clone(),
            model_cache.clone(),
            provider_cache.clone(),
            ssrf_dns_cache.clone(),
            pinned_clients.clone(),
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
            shutdown_token,
            api_key_cache,
            model_cache,
            provider_cache,
            ssrf_dns_cache,
            pinned_clients,
            dlp,
        })
    }
}

/// Evict expired in-memory cache entries at a configurable interval.
/// Applies a shorter TTL when a cache exceeds 90% of max_entries.
/// Covers api_key, model, provider, and SSRF DNS caches in one task.
/// Note: `max_entries` is only a soft threshold here; the model cache also
/// enforces it as a hard cap at insertion time (see insert_model_cache).
#[allow(clippy::too_many_arguments)]
fn spawn_caches_cleanup(
    api_key_cache: Arc<DashMap<String, (Option<ApiKeyInfo>, Instant)>>,
    model_cache: Arc<DashMap<String, ModelCacheEntry>>,
    provider_cache: Arc<DashMap<String, ProviderCacheEntry>>,
    ssrf_dns_cache: Arc<DashMap<String, (Vec<IpAddr>, Instant)>>,
    pinned_clients: Arc<DashMap<String, (reqwest::Client, Instant)>>,
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
                        api_key_cache.clone(),
                        model_cache.clone(),
                        provider_cache.clone(),
                        ssrf_dns_cache.clone(),
                    );
                    if let Err(e) = crate::run_blocking(move || {
                        cleanup_caches(&caches.0, &caches.1, &caches.2, &caches.3, max_entries)
                    }).await {
                        tracing::warn!("Cache cleanup error: {e}");
                    }
                    // Drop pinned SSRF clients past TTL so deleted providers and
                    // changed DNS eventually release their pooled connections.
                    pinned_clients.retain(|_, v| v.1.elapsed() < CACHE_TTL);
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
    api_key_cache: &DashMap<String, (Option<ApiKeyInfo>, Instant)>,
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
    let ttl = aggressive(api_key_cache.len());
    // Negative entries (known-invalid tokens) use the short negative TTL so a
    // miss flood is evicted quickly instead of crowding out positive entries.
    api_key_cache.retain(|_, v| {
        let entry_ttl = if v.0.is_none() {
            NEGATIVE_CACHE_TTL
        } else {
            ttl
        };
        v.1.elapsed() < entry_ttl
    });
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

#[cfg(test)]
mod tests {
    use super::*;

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

    fn test_api_key_info() -> ApiKeyInfo {
        ApiKeyInfo {
            id: "key-1".to_string(),
            name: "test-key".to_string(),
            enabled: true,
            expires_at: None,
            created_at: chrono::Utc::now(),
        }
    }

    fn new_api_key_cache() -> Arc<DashMap<String, (Option<ApiKeyInfo>, Instant)>> {
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
        let api_key_cache = new_api_key_cache();
        let model_cache = new_model_cache();
        let provider_cache = new_provider_cache();
        let ssrf_cache = new_ssrf_cache();

        api_key_cache.insert("k-old".to_string(), (Some(test_api_key_info()), stale()));
        api_key_cache.insert("k-new".to_string(), (Some(test_api_key_info()), fresh()));
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
            let api_key_cache = api_key_cache.clone();
            let model_cache = model_cache.clone();
            let provider_cache = provider_cache.clone();
            let ssrf_cache = ssrf_cache.clone();
            move || {
                cleanup_caches(
                    &api_key_cache,
                    &model_cache,
                    &provider_cache,
                    &ssrf_cache,
                    1000,
                );
            }
        });

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
        let api_key_cache = new_api_key_cache();
        let model_cache = new_model_cache();
        let provider_cache = new_provider_cache();
        let ssrf_cache = new_ssrf_cache();

        // 200s-old entries survive the normal 300s TTL but not the aggressive
        // 150s TTL triggered when len() > 90% of max_entries.
        let mid_old = Instant::now() - Duration::from_secs(200);

        // Insert 10 entries in api_key_cache so len() > threshold (9),
        // triggering aggressive TTL = CACHE_TTL/2 = 150s for that cache.
        for i in 0..10 {
            api_key_cache.insert(format!("k{i}"), (Some(test_api_key_info()), mid_old));
        }
        // Negative entries take the short NEGATIVE_CACHE_TTL regardless of the
        // aggressive TTL, so this 200s-old one is evicted either way.
        api_key_cache.insert("k-neg".to_string(), (None, mid_old));
        // Other caches have only 1 entry each -- normal 300s TTL applies.
        model_cache.insert("m0".to_string(), (None, mid_old));
        provider_cache.insert("p0".to_string(), (Arc::new(MockProvider), mid_old));
        ssrf_cache.insert(
            "d0".to_string(),
            (vec!["1.2.3.4".parse().unwrap()], mid_old),
        );

        assert_no_deadlock(Duration::from_secs(5), {
            let api_key_cache = api_key_cache.clone();
            let model_cache = model_cache.clone();
            let provider_cache = provider_cache.clone();
            let ssrf_cache = ssrf_cache.clone();
            move || {
                cleanup_caches(
                    &api_key_cache,
                    &model_cache,
                    &provider_cache,
                    &ssrf_cache,
                    10,
                );
            }
        });

        // api_key_cache: len was 10 > threshold 9 -> aggressive TTL 150s ->
        // all 200s-old entries evicted.
        assert!(api_key_cache.is_empty());
        // model_cache: negative entry, NEGATIVE_CACHE_TTL=30s -> evicted.
        assert!(model_cache.is_empty());
        // provider_cache: len=1, normal 300s TTL, 200s < 300s -> survives.
        assert_eq!(provider_cache.len(), 1);
        // ssrf_cache: len=1, normal 300s TTL, 200s < 300s -> survives.
        assert_eq!(ssrf_cache.len(), 1);
    }

    #[test]
    fn cleanup_caches_with_concurrent_writers_no_deadlock() {
        let api_key_cache = new_api_key_cache();
        let model_cache = new_model_cache();
        let provider_cache = new_provider_cache();
        let ssrf_cache = new_ssrf_cache();
        // Seed entries so the cleanup task has something to walk.
        for i in 0..50 {
            api_key_cache.insert(format!("k{i}"), (Some(test_api_key_info()), fresh()));
        }

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
                let api_key_cache = api_key_cache.clone();
                let model_cache = model_cache.clone();
                let provider_cache = provider_cache.clone();
                let ssrf_cache = ssrf_cache.clone();
                handles.push(std::thread::spawn(move || {
                    for i in 0..200 {
                        let key = format!("w{i}");
                        api_key_cache.insert(key.clone(), (Some(test_api_key_info()), fresh()));
                        model_cache.insert(key.clone(), (None, fresh()));
                        provider_cache.insert(key.clone(), (Arc::new(MockProvider), fresh()));
                        ssrf_cache.insert(key.clone(), (vec!["1.2.3.4".parse().unwrap()], fresh()));
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

    #[tokio::test]
    async fn app_state_new_initializes_all_fields() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let log_path = dir.path().join("test.duckdb");
        let config =
            crate::test_utils::test_config(db_path.to_str().unwrap(), log_path.to_str().unwrap());
        let state = AppState::new(config).unwrap();
        assert!(state.config.auth.enabled);
        assert!(state.api_key_cache.is_empty());
        assert!(state.model_cache.is_empty());
        assert!(state.provider_cache.is_empty());
        assert!(state.ssrf_dns_cache.is_empty());
        assert!(state.pinned_clients.is_empty());
        state.shutdown_token.cancel();
    }
}
