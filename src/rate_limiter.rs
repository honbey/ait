use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;

#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<DashMap<IpAddr, RateEntry>>,
}

#[derive(Clone)]
struct RateEntry {
    attempts: Vec<Instant>,
    banned_until: Option<Instant>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
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

    pub fn cleanup(&self) {
        let now = Instant::now();
        self.inner.retain(|_, entry| {
            if let Some(banned_until) = entry.banned_until {
                if now >= banned_until {
                    return false;
                }
                return true;
            }
            let cutoff = now - Duration::from_secs(3600);
            entry.attempts.retain(|&t| t > cutoff);
            !entry.attempts.is_empty()
        });
    }
}
