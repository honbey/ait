//! Model and provider resolution for the proxy hot path.
//!
//! Kept apart from the request flow in `mod.rs`: three bounded caches, each
//! with its own eviction and negative-caching rules, are enough to deserve a
//! module, and `proxy_request` was unreadable with them inline.

use std::sync::Arc;
use std::time::Instant;

use axum::{Json, http::StatusCode};

use crate::app::{AppState, NEGATIVE_CACHE_TTL};
use crate::db::{Model, Provider};
use crate::error::{AitError, internal_error, not_found};
use crate::middleware::CACHE_TTL;
use crate::providers::{UpstreamProvider, create_provider};

/// Cache entries that know when they were written.
///
/// `insert_capped` is generic over the value type, so it cannot reach a
/// timestamp on its own; the caches that go through it expose theirs here.
trait TimestampedEntry {
    fn inserted_at(&self) -> Instant;
}

impl TimestampedEntry for Instant {
    fn inserted_at(&self) -> Instant {
        *self
    }
}

impl<T> TimestampedEntry for (T, Instant) {
    fn inserted_at(&self) -> Instant {
        self.1
    }
}

/// Insert into a cache, evicting the oldest entry once the cap is reached.
///
/// Dropping the new entry instead would stop caching altogether: hot entries
/// slide their timestamp on every hit (see `resolve_model_and_provider`), so
/// they never age out during cleanup and the cache stays pinned at the cap
/// forever.
///
/// `DashMap::len` is approximate under concurrency, so a cache stays bounded
/// within a small multiple of `cache_max_entries`.
///
/// Shared by the model cache and the negative model cache (keys come from
/// request bodies and are therefore attacker-controlled) and the provider
/// cache, so all three enforce the cap.
fn insert_capped<V: TimestampedEntry>(
    map: &dashmap::DashMap<String, V>,
    key: &str,
    entry: V,
    max_entries: usize,
) {
    if max_entries == 0 {
        return;
    }
    // Refreshing an existing key must not count against the cap.
    if map.contains_key(key) {
        map.insert(key.to_string(), entry);
        return;
    }
    if map.len() >= max_entries {
        // Clone the key and let the iterator (and the shard read lock it
        // holds) go before removing; parking_lot's RwLock is not reentrant.
        let oldest = map
            .iter()
            .min_by_key(|e| e.value().inserted_at())
            .map(|e| e.key().clone());
        if let Some(oldest) = oldest {
            map.remove(&oldest);
        }
    }
    map.insert(key.to_string(), entry);
}

/// Resolve the model the client named, and the provider that serves it.
///
/// Unknown names are cached separately from resolved ones: sharing
/// `model_cache` let a flood of bogus names fill the entry cap, after which
/// valid models were no longer cached either and every request fell through to
/// a blocking SQLite lookup.
pub(crate) async fn resolve_model_and_provider(
    state: &AppState,
    model_name: &str,
) -> Result<(Model, Provider), (StatusCode, Json<AitError>)> {
    // Known-unknown first: it costs one lookup and keeps the negative verdict
    // out of `model_cache` entirely.
    if state
        .negative_model_cache
        .get(model_name)
        .is_some_and(|seen| seen.elapsed() < NEGATIVE_CACHE_TTL)
    {
        return Err(not_found(format!(
            "Model '{}' not found or disabled",
            model_name
        )));
    }

    let cached = state.model_cache.get_mut(model_name).and_then(|mut entry| {
        if entry.1.elapsed() < CACHE_TTL {
            // slide positive entries so hot models stay cached
            entry.1 = Instant::now();
            Some(entry.0.clone())
        } else {
            None
        }
    });
    if let Some(found) = cached {
        return Ok(found);
    }

    let db = state.db.clone();
    let name = model_name.to_string();
    let max_entries = state.config.server.cache_max_entries as usize;
    match crate::run_blocking(move || db.resolve_model(&name)).await {
        Ok(Ok(Some((m, p)))) => {
            let upstream = create_provider(&p, state.http_client.clone());
            insert_capped(
                &state.provider_cache,
                &p.id,
                (upstream, Instant::now()),
                max_entries,
            );
            insert_capped(
                &state.model_cache,
                model_name,
                ((m.clone(), p.clone()), Instant::now()),
                max_entries,
            );
            Ok((m, p))
        }
        Ok(Ok(None)) => {
            insert_capped(
                &state.negative_model_cache,
                model_name,
                Instant::now(),
                max_entries,
            );
            Err(not_found(format!(
                "Model '{}' not found or disabled",
                model_name
            )))
        }
        Ok(Err(e)) => Err(AitError::from_db_error(e).into_response()),
        Err(join_err) => Err(internal_error(join_err)),
    }
}

/// The upstream client for `provider`, reusing the cached one while fresh.
pub(crate) fn cached_upstream(state: &AppState, provider: &Provider) -> Arc<dyn UpstreamProvider> {
    let max_entries = state.config.server.cache_max_entries as usize;
    let cached = state
        .provider_cache
        .get_mut(&provider.id)
        .and_then(|mut entry| {
            if entry.1.elapsed() < CACHE_TTL {
                entry.1 = Instant::now();
                Some(entry.0.clone())
            } else {
                None
            }
        });
    match cached {
        Some(upstream) => upstream,
        None => {
            let upstream = create_provider(provider, state.http_client.clone());
            insert_capped(
                &state.provider_cache,
                &provider.id,
                (upstream.clone(), Instant::now()),
                max_entries,
            );
            upstream
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

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

    #[test]
    fn insert_capped_respects_cap() {
        let map: dashmap::DashMap<String, Instant> = dashmap::DashMap::new();
        let entry = Instant::now();
        for i in 0..5 {
            insert_capped(&map, &format!("m{i}"), entry, 5);
        }
        assert_eq!(map.len(), 5);
        insert_capped(&map, "m6", entry, 5);
        assert_eq!(map.len(), 5);
    }

    #[test]
    fn insert_capped_zero_cap_caches_nothing() {
        let map: dashmap::DashMap<String, Instant> = dashmap::DashMap::new();
        insert_capped(&map, "m0", Instant::now(), 0);
        assert!(map.is_empty());
    }

    #[test]
    fn insert_capped_evicts_oldest_when_full() {
        let map: dashmap::DashMap<String, Instant> = dashmap::DashMap::new();
        let start = Instant::now();
        for i in 0..3 {
            insert_capped(&map, &format!("m{i}"), start + Duration::from_secs(i), 3);
        }
        // A full cache must keep caching: the new entry replaces the oldest
        // instead of being dropped, which is what kept caching off entirely.
        insert_capped(&map, "m3", start + Duration::from_secs(3), 3);
        assert_eq!(map.len(), 3);
        assert!(!map.contains_key("m0"), "oldest entry is evicted");
        assert!(map.contains_key("m3"), "newest entry is kept");
    }

    #[test]
    fn insert_capped_eviction_survives_concurrency() {
        // Eviction scans for the oldest entry, then removes it - the shape
        // AGENTS.md warns about. It has to stay correct with other threads
        // inserting and evicting at the same time.
        let map: dashmap::DashMap<String, Instant> = dashmap::DashMap::new();
        const CAP: usize = 8;

        crate::test_utils::assert_no_deadlock(std::time::Duration::from_secs(10), {
            let map = map.clone();
            move || {
                let writers: Vec<_> = (0..4)
                    .map(|thread| {
                        let map = map.clone();
                        std::thread::spawn(move || {
                            for i in 0..200 {
                                insert_capped(&map, &format!("k{thread}-{i}"), Instant::now(), CAP);
                            }
                        })
                    })
                    .collect();
                for writer in writers {
                    writer.join().unwrap();
                }
            }
        });

        // DashMap's len is approximate under concurrency, so allow slack -
        // what matters is that the cap held.
        assert!(
            map.len() <= CAP * 2,
            "cache grew past its cap: {} entries",
            map.len()
        );
    }

    #[test]
    fn capped_eviction_and_cache_cleanup_do_not_deadlock() {
        // Two different guard patterns on the same maps at once: the periodic
        // cleanup's `retain`, and `insert_capped`'s scan-then-remove. Neither
        // may hold a guard across the other's insert/retain/remove.
        let api_key_cache: dashmap::DashMap<String, (crate::db::ApiKeyInfo, Instant)> =
            dashmap::DashMap::new();
        let model_cache: dashmap::DashMap<String, crate::app::ModelCacheEntry> =
            dashmap::DashMap::new();
        let provider_cache: dashmap::DashMap<String, (Arc<dyn UpstreamProvider>, Instant)> =
            dashmap::DashMap::new();
        let ssrf_cache: dashmap::DashMap<String, (Vec<std::net::IpAddr>, Instant)> =
            dashmap::DashMap::new();
        const MAX: usize = 20;

        let entry = || {
            (
                (
                    crate::test_utils::create_test_model("m", "p"),
                    crate::test_utils::create_test_provider(
                        "p",
                        crate::db::ProviderType::OpenAICompat,
                        "http://127.0.0.1:1",
                    ),
                ),
                Instant::now(),
            )
        };

        let evict_model = model_cache.clone();
        let evict_provider = provider_cache.clone();
        crate::test_utils::assert_no_deadlock(std::time::Duration::from_secs(10), move || {
            let evictor = std::thread::spawn(move || {
                for i in 0..200 {
                    insert_capped(&evict_model, &format!("mx{i}"), entry(), MAX);
                    insert_capped(
                        &evict_provider,
                        &format!("px{i}"),
                        (Arc::new(MockProvider), Instant::now()),
                        MAX,
                    );
                }
            });
            for _ in 0..200 {
                crate::app::cleanup_caches(
                    &api_key_cache,
                    &model_cache,
                    &provider_cache,
                    &ssrf_cache,
                    MAX,
                );
            }
            evictor.join().unwrap();
        });
    }

    #[test]
    fn insert_capped_bounds_provider_cache() {
        struct NoopProvider;

        #[async_trait::async_trait]
        impl UpstreamProvider for NoopProvider {
            async fn build_request(
                &self,
                _client: &reqwest::Client,
                _body: serde_json::Value,
                _stream: bool,
                _upstream_model: &str,
                _upstream_path: &str,
            ) -> Result<reqwest::Request, String> {
                Err("noop provider is not usable".to_string())
            }
        }

        let map: dashmap::DashMap<String, (Arc<dyn UpstreamProvider>, Instant)> =
            dashmap::DashMap::new();
        let entry = || {
            (
                Arc::new(NoopProvider) as Arc<dyn UpstreamProvider>,
                Instant::now(),
            )
        };
        for i in 0..5 {
            insert_capped(&map, &format!("p{i}"), entry(), 5);
        }
        assert_eq!(map.len(), 5);
        insert_capped(&map, "p6", entry(), 5);
        assert_eq!(map.len(), 5, "provider cache must honour the same cap");
    }
}
