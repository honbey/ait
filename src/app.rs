use crate::config::ConfigApp;
use crate::db::Database;
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

        Self {
            config,
            db,
            http_client,
            start_time: Utc::now(),
        }
    }
}