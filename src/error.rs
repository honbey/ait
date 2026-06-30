use axum::{Json, http::StatusCode};
use serde::Serialize;

use crate::db::DbError;

/// Errors that can occur during `AppState::new` initialization.
///
/// Returned to `main` so the process can log a precise message and exit once,
/// rather than `process::exit` being scattered inside the constructor.
#[derive(Debug)]
pub enum AppInitError {
    Database(Box<dyn std::error::Error + Send + Sync>),
    HttpClient(reqwest::Error),
    /// `create_user` returns `Result<_, String>` (handlers/users.rs).
    BootstrapUser(String),
    LogManager(duckdb::Error),
}

impl std::fmt::Display for AppInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppInitError::Database(e) => write!(f, "failed to open database: {}", e),
            AppInitError::HttpClient(e) => write!(f, "failed to build HTTP client: {}", e),
            AppInitError::BootstrapUser(msg) => {
                write!(f, "failed to bootstrap initial user: {}", msg)
            }
            AppInitError::LogManager(e) => write!(f, "failed to initialize log database: {}", e),
        }
    }
}

impl std::error::Error for AppInitError {}

#[derive(Debug, Serialize)]
pub struct AitError {
    pub message: String,
    pub code: u16,
    pub r#type: String,
}

impl AitError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            code: 400,
            r#type: "invalid_request_error".to_string(),
        }
    }

    pub fn upstream_error(status: u16, msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            code: status,
            r#type: "upstream_error".to_string(),
        }
    }

    pub fn status_code(&self) -> StatusCode {
        StatusCode::from_u16(self.code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
    }

    pub fn from_db_error(e: DbError) -> Self {
        match e {
            DbError::NotFound(msg) => Self {
                message: msg,
                code: 404,
                r#type: "not_found_error".to_string(),
            },
            DbError::LimitExceeded(msg) => Self {
                message: msg,
                code: 409,
                r#type: "invalid_request_error".to_string(),
            },
            DbError::Storage(msg) => Self {
                message: msg,
                code: 500,
                r#type: "internal_error".to_string(),
            },
        }
    }

    pub fn into_response(self) -> (StatusCode, Json<AitError>) {
        (self.status_code(), Json(self))
    }
}

impl From<AitError> for (StatusCode, Json<AitError>) {
    fn from(err: AitError) -> Self {
        err.into_response()
    }
}

impl From<DbError> for (StatusCode, Json<AitError>) {
    fn from(e: DbError) -> Self {
        AitError::from_db_error(e).into_response()
    }
}

// --- HTTP response helpers ---

pub fn internal_error(e: impl std::fmt::Display) -> (StatusCode, Json<AitError>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(AitError {
            message: e.to_string(),
            code: 500,
            r#type: "internal_error".to_string(),
        }),
    )
}

pub fn not_found(msg: impl Into<String>) -> (StatusCode, Json<AitError>) {
    (
        StatusCode::NOT_FOUND,
        Json(AitError {
            message: msg.into(),
            code: 404,
            r#type: "not_found_error".to_string(),
        }),
    )
}

pub fn forbidden(msg: impl Into<String>) -> (StatusCode, Json<AitError>) {
    (
        StatusCode::FORBIDDEN,
        Json(AitError {
            message: msg.into(),
            code: 403,
            r#type: "forbidden".to_string(),
        }),
    )
}

pub fn unauthorized(msg: impl Into<String>) -> (StatusCode, Json<AitError>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(AitError {
            message: msg.into(),
            code: 401,
            r#type: "auth_error".to_string(),
        }),
    )
}

pub fn conflict(msg: impl Into<String>) -> (StatusCode, Json<AitError>) {
    (
        StatusCode::CONFLICT,
        Json(AitError {
            message: msg.into(),
            code: 409,
            r#type: "invalid_request_error".to_string(),
        }),
    )
}

pub fn too_many_requests(msg: impl Into<String>) -> (StatusCode, Json<AitError>) {
    (
        StatusCode::TOO_MANY_REQUESTS,
        Json(AitError {
            message: msg.into(),
            code: 429,
            r#type: "rate_limit_error".to_string(),
        }),
    )
}

pub fn db_error() -> (StatusCode, Json<AitError>) {
    internal_error("Database error")
}
