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
    LogManager(duckdb::Error),
}

impl std::fmt::Display for AppInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppInitError::Database(e) => write!(f, "failed to open database: {}", e),
            AppInitError::HttpClient(e) => write!(f, "failed to build HTTP client: {}", e),
            AppInitError::LogManager(e) => write!(f, "failed to initialize log database: {}", e),
        }
    }
}

impl std::error::Error for AppInitError {}

#[derive(Debug)]
pub enum BlockingError {
    Join(tokio::task::JoinError),
    Timeout,
}

impl std::fmt::Display for BlockingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockingError::Join(e) => write!(f, "blocking task failed: {}", e),
            BlockingError::Timeout => write!(f, "blocking task timed out after 30s"),
        }
    }
}

impl std::error::Error for BlockingError {}

#[derive(Debug, Serialize)]
pub struct AitError {
    pub message: String,
    pub code: u16,
    pub r#type: String,
    /// Internal detail (e.g. upstream error body) recorded in logs only;
    /// never serialized to clients.
    #[serde(skip_serializing)]
    pub detail: Option<String>,
}

impl AitError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            code: 400,
            r#type: "invalid_request_error".to_string(),
            detail: None,
        }
    }

    pub fn upstream_error(status: u16, msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            code: status,
            r#type: "upstream_error".to_string(),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
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
                detail: None,
            },
            DbError::LimitExceeded(msg) => Self {
                message: msg,
                code: 409,
                r#type: "invalid_request_error".to_string(),
                detail: None,
            },
            DbError::Duplicate(msg) => Self {
                message: msg,
                code: 409,
                r#type: "invalid_request_error".to_string(),
                detail: None,
            },
            DbError::Storage(msg) => Self {
                message: msg,
                code: 500,
                r#type: "internal_error".to_string(),
                detail: None,
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
    tracing::error!("Internal error: {}", e);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(AitError {
            message: "Internal server error".to_string(),
            code: 500,
            r#type: "internal_error".to_string(),
            detail: None,
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
            detail: None,
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
            detail: None,
        }),
    )
}

pub fn db_error() -> (StatusCode, Json<AitError>) {
    internal_error("Database error")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_constructors_have_correct_shape() {
        type ErrorCtor = fn(&str) -> (StatusCode, Json<AitError>);
        let cases: Vec<(ErrorCtor, StatusCode, u16, &str)> = vec![
            (
                |m| AitError::bad_request(m).into_response(),
                StatusCode::BAD_REQUEST,
                400,
                "invalid_request_error",
            ),
            (
                |m| not_found(m),
                StatusCode::NOT_FOUND,
                404,
                "not_found_error",
            ),
            (
                |m| unauthorized(m),
                StatusCode::UNAUTHORIZED,
                401,
                "auth_error",
            ),
        ];
        for (ctor, status, code, ty) in cases {
            let (s, body) = ctor("some message");
            assert_eq!(s, status);
            assert_eq!(body.0.code, code);
            assert_eq!(body.0.r#type, ty);
            assert_eq!(body.0.message, "some message");
            assert_eq!(body.0.status_code(), status);
        }
    }

    #[test]
    fn internal_error_uses_generic_message() {
        let (s, body) = internal_error("secret detail");
        assert_eq!(s, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body.0.code, 500);
        assert_eq!(body.0.message, "Internal server error");
        assert_eq!(body.0.r#type, "internal_error");
    }

    #[test]
    fn db_error_maps_to_internal_error() {
        let (s, body) = db_error();
        assert_eq!(s, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body.0.code, 500);
    }

    #[test]
    fn from_db_error_maps_all_variants() {
        let cases = vec![
            (DbError::NotFound("gone".into()), 404, "not_found_error"),
            (
                DbError::LimitExceeded("too many".into()),
                409,
                "invalid_request_error",
            ),
            (
                DbError::Duplicate("dup".into()),
                409,
                "invalid_request_error",
            ),
            (DbError::Storage("boom".into()), 500, "internal_error"),
        ];
        for (e, code, ty) in cases {
            let err = AitError::from_db_error(e);
            assert_eq!(err.code, code);
            assert_eq!(err.r#type, ty);
            assert_eq!(err.status_code().as_u16(), code);
        }
    }

    #[test]
    fn app_init_error_display_variants() {
        let db_err = AppInitError::Database(Box::new(std::io::Error::other("disk full")));
        assert!(db_err.to_string().contains("failed to open database"));

        let log_err = AppInitError::LogManager(duckdb::Error::InvalidParameterName("bad".into()));
        assert!(
            log_err
                .to_string()
                .contains("failed to initialize log database")
        );
    }

    #[tokio::test]
    async fn app_init_error_http_client_display() {
        // Connection refused on a closed local port yields a real reqwest::Error.
        let err = reqwest::Client::new()
            .get("http://127.0.0.1:1/")
            .send()
            .await
            .unwrap_err();
        let http_err = AppInitError::HttpClient(err);
        assert!(http_err.to_string().contains("failed to build HTTP client"));
    }

    #[test]
    fn blocking_error_timeout_display() {
        assert!(BlockingError::Timeout.to_string().contains("timed out"));
    }

    #[tokio::test]
    async fn blocking_error_join_display() {
        let handle = tokio::task::spawn(async { panic!("boom") });
        let join = BlockingError::Join(handle.await.unwrap_err());
        assert!(join.to_string().contains("panicked"));
    }
}
