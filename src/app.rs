use crate::config::ConfigApp;
use crate::db::{Database, User, UserRole};
use chrono::{DateTime, Utc};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub config: ConfigApp,
    pub db: Arc<Database>,
    pub http_client: reqwest::Client,
    pub start_time: DateTime<Utc>,
}

impl AppState {
    pub fn new(config: ConfigApp) -> Self {
        let db = match Database::new(&config.database.path) {
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
                let password_hash = bcrypt::hash(&config.auth.bootstrap_password, bcrypt::DEFAULT_COST)
                    .expect("Failed to hash bootstrap password");
                let user = User {
                    username: config.auth.bootstrap_username.clone(),
                    password_hash,
                    role: UserRole::Admin,
                    allowed: vec![],
                    created_at: Utc::now(),
                };
                db.insert_user(user).expect("Failed to bootstrap admin user");
                tracing::info!("Bootstrapped admin user '{}'", config.auth.bootstrap_username);
            }
        }

        Self {
            config,
            db,
            http_client,
            start_time: Utc::now(),
        }
    }
}