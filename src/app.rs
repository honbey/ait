use crate::config::ConfigApp;
use crate::db::{Database, User, UserRole};
use crate::rate_limiter::RateLimiter;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct AppState {
    pub config: ConfigApp,
    pub db: Arc<Database>,
    pub http_client: reqwest::Client,
    pub start_time: DateTime<Utc>,
    pub login_rate_limiter: RateLimiter,
    pub register_rate_limiter: RateLimiter,
}

impl AppState {
    pub fn new(config: ConfigApp) -> Self {
        let db = match Database::new(&config.database.path, config.auth.max_api_keys_per_user) {
            Ok(d) => Arc::new(d),
            Err(e) => {
                eprintln!("Failed to open database: {}", e);
                std::process::exit(1);
            }
        };

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.proxy.timeout_secs))
            .build()
            .expect("Failed to build HTTP client");

        // Bootstrap admin user on first startup if configured
        if config.auth.bootstrap_admin {
            let users = db.list_users().unwrap_or_default();
            if users.is_empty() {
                let password_hash =
                    bcrypt::hash(&config.auth.bootstrap_password, bcrypt::DEFAULT_COST)
                        .expect("Failed to hash bootstrap password");
                let user = User {
                    username: config.auth.bootstrap_username.clone(),
                    password_hash,
                    role: UserRole::Admin,
                    allowed: vec![],
                    api_keys: vec![],
                    created_at: Utc::now(),
                    updated_at: Default::default(),
                };
                db.insert_user(user)
                    .expect("Failed to bootstrap admin user");
                tracing::info!(
                    "Bootstrapped admin user '{}'",
                    config.auth.bootstrap_username
                );
            }
        }

        // Periodic cleanup of expired sessions
        let interval_secs = config.server.session_cleanup_interval_secs;
        let cleanup_db = db.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            loop {
                interval.tick().await;
                match cleanup_db.cleanup_expired_sessions() {
                    Ok(count) => {
                        if count > 0 {
                            tracing::info!("Cleaned up {} expired sessions", count);
                        }
                    }
                    Err(e) => tracing::error!("Session cleanup error: {}", e),
                }
            }
        });

        // Rate limiter cleanup tasks
        let login_limiter = RateLimiter::new();
        let register_limiter = RateLimiter::new();
        let cleanup_interval = config.server.rate_limiter_cleanup_interval_secs;
        spawn_rate_limiter_cleanup(login_limiter.clone(), cleanup_interval);
        spawn_rate_limiter_cleanup(register_limiter.clone(), cleanup_interval);

        Self {
            config,
            db,
            http_client,
            start_time: Utc::now(),
            login_rate_limiter: login_limiter,
            register_rate_limiter: register_limiter,
        }
    }
}

fn spawn_rate_limiter_cleanup(limiter: RateLimiter, interval_secs: u64) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
        loop {
            interval.tick().await;
            limiter.cleanup();
        }
    });
}
