use axum::{Json, http::StatusCode};
use serde::Serialize;

use crate::db::{DbError, SessionUser, UserRole};

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

    pub fn unauthorized() -> Self {
        Self {
            message: "Unauthorized: invalid or missing API key".to_string(),
            code: 401,
            r#type: "auth_error".to_string(),
        }
    }

    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            code: 403,
            r#type: "forbidden".to_string(),
        }
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            code: 404,
            r#type: "not_found_error".to_string(),
        }
    }

    pub fn conflict(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            code: 409,
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

    pub fn internal_error(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            code: 500,
            r#type: "internal_error".to_string(),
        }
    }

    pub fn status_code(&self) -> StatusCode {
        StatusCode::from_u16(self.code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
    }

    pub fn from_db_error(e: DbError) -> Self {
        match e {
            DbError::NotFound(msg) => Self::not_found(msg),
            DbError::LimitExceeded(msg) => Self::conflict(msg),
            DbError::Storage(msg) => Self::internal_error(msg),
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
    AitError::internal_error(e.to_string()).into_response()
}

pub fn not_found(msg: impl Into<String>) -> (StatusCode, Json<AitError>) {
    AitError::not_found(msg).into_response()
}

pub fn forbidden(msg: impl Into<String>) -> (StatusCode, Json<AitError>) {
    AitError::forbidden(msg).into_response()
}

pub fn unauthorized(msg: impl Into<String>) -> (StatusCode, Json<AitError>) {
    AitError {
        message: msg.into(),
        code: 401,
        r#type: "auth_error".to_string(),
    }
    .into_response()
}

pub fn conflict(msg: impl Into<String>) -> (StatusCode, Json<AitError>) {
    AitError::conflict(msg).into_response()
}

pub fn db_error() -> (StatusCode, Json<AitError>) {
    AitError::internal_error("Database error").into_response()
}

pub fn require_admin(session: &SessionUser) -> Result<(), (StatusCode, Json<AitError>)> {
    if session.role != UserRole::Admin {
        return Err(forbidden("Admin privileges required"));
    }
    Ok(())
}

pub fn require_admin_or_self(
    session: &SessionUser,
    username: &str,
) -> Result<(), (StatusCode, Json<AitError>)> {
    if session.role != UserRole::Admin && session.username != username {
        return Err(forbidden("Admin privileges required"));
    }
    Ok(())
}
