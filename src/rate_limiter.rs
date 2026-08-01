use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;

#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<DashMap<IpAddr, RateEntry>>,
    max_entries: usize,
}

#[derive(Clone)]
struct RateEntry {
    attempts: Vec<Instant>,
    banned_until: Option<Instant>,
}

impl RateLimiter {
    pub fn new(max_entries: usize) -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
            max_entries,
        }
    }

    pub fn check_and_record(
        &self,
        ip: IpAddr,
        max_attempts: u64,
        window_secs: u64,
        ban_secs: u64,
    ) -> Result<(), Duration> {
        let now = Instant::now();

        if !self.inner.contains_key(&ip) && self.inner.len() >= self.max_entries {
            // Extract the eviction key first: the DashMap iterator holds a read
            // lock, and parking_lot locks are not reentrant, so removing while
            // the iterator is still in scope would deadlock.
            let evict = self.inner.iter().next().map(|entry| *entry.key());
            if let Some(key) = evict {
                tracing::warn!(evicted = %key, size = %self.inner.len(), "rate limiter at capacity, evicting entry");
                self.inner.remove(&key);
            }
        }

        let mut entry = self.inner.entry(ip).or_insert(RateEntry {
            attempts: Vec::new(),
            banned_until: None,
        });

        if let Some(banned_until) = entry.banned_until {
            if now < banned_until {
                return Err(banned_until - now);
            }
            entry.banned_until = None;
            entry.attempts.clear();
        }

        let cutoff = now - Duration::from_secs(window_secs);
        entry.attempts.retain(|&t| t > cutoff);

        if entry.attempts.len() >= max_attempts as usize {
            entry.banned_until = Some(now + Duration::from_secs(ban_secs));
            return Err(Duration::from_secs(ban_secs));
        }

        entry.attempts.push(now);
        Ok(())
    }

    pub fn clear(&self, ip: IpAddr) {
        self.inner.remove(&ip);
    }

    pub fn cleanup(&self, max_window_secs: u64) {
        let now = Instant::now();
        self.inner.retain(|_, entry| {
            if let Some(banned_until) = entry.banned_until {
                if now >= banned_until {
                    return false;
                }
                return true;
            }
            let cutoff = now - Duration::from_secs(max_window_secs);
            entry.attempts.retain(|&t| t > cutoff);
            !entry.attempts.is_empty()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(n: u8) -> IpAddr {
        IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, n))
    }

    #[test]
    fn allows_within_limit_and_records_attempts() {
        let limiter = RateLimiter::new(10);
        // The check happens before recording, so the (max_attempts + 1)th
        // request is the first one rejected.
        for _ in 0..5 {
            assert!(limiter.check_and_record(ip(1), 5, 60, 60).is_ok());
        }
        assert!(limiter.check_and_record(ip(1), 5, 60, 60).is_err());
    }

    #[test]
    fn ban_expires_and_allows_again() {
        let limiter = RateLimiter::new(10);
        for _ in 0..5 {
            let _ = limiter.check_and_record(ip(1), 5, 60, 1);
        }
        assert!(limiter.check_and_record(ip(1), 5, 60, 1).is_err());
        std::thread::sleep(Duration::from_millis(1100));
        assert!(limiter.check_and_record(ip(1), 5, 60, 1).is_ok());
    }

    #[test]
    fn window_rollover_resets_attempts() {
        let limiter = RateLimiter::new(10);
        // 5 attempts inside the 1s window, below the limit of 5.
        for _ in 0..5 {
            assert!(limiter.check_and_record(ip(1), 5, 1, 60).is_ok());
        }
        std::thread::sleep(Duration::from_millis(1100));
        // Old attempts fell out of the window, so the count restarts: 5 more
        // are allowed before the 6th trips the limit again.
        for _ in 0..5 {
            assert!(limiter.check_and_record(ip(1), 5, 1, 60).is_ok());
        }
        assert!(limiter.check_and_record(ip(1), 5, 1, 60).is_err());
    }

    #[test]
    fn clear_resets_entry() {
        let limiter = RateLimiter::new(10);
        for _ in 0..5 {
            let _ = limiter.check_and_record(ip(1), 5, 60, 60);
        }
        assert!(limiter.check_and_record(ip(1), 5, 60, 60).is_err());
        limiter.clear(ip(1));
        assert!(limiter.check_and_record(ip(1), 5, 60, 60).is_ok());
    }

    #[test]
    fn cleanup_removes_stale_entries() {
        let limiter = RateLimiter::new(10);
        let _ = limiter.check_and_record(ip(1), 5, 1, 60);
        limiter.cleanup(1);
        assert_eq!(limiter.inner.len(), 1);
        std::thread::sleep(Duration::from_millis(1100));
        limiter.cleanup(1);
        assert_eq!(limiter.inner.len(), 0);
    }

    #[test]
    fn evicts_entry_when_at_capacity() {
        let limiter = RateLimiter::new(1);
        assert!(limiter.check_and_record(ip(1), 5, 60, 60).is_ok());
        // New IP at capacity evicts the old entry instead of failing.
        assert!(limiter.check_and_record(ip(2), 5, 60, 60).is_ok());
        assert_eq!(limiter.inner.len(), 1);
    }
}
