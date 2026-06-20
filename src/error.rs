use axum::{Json, http::StatusCode};
use serde::Serialize;

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

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            code: 404,
            r#type: "not_found_error".to_string(),
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
}

// --- HTTP response helpers ---

pub fn internal_error(e: impl std::fmt::Display) -> (StatusCode, Json<AitError>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(AitError::internal_error(e.to_string())),
    )
}

pub fn not_found(msg: impl Into<String>) -> (StatusCode, Json<AitError>) {
    (StatusCode::NOT_FOUND, Json(AitError::not_found(msg)))
}

pub fn forbidden() -> (StatusCode, Json<AitError>) {
    (
        StatusCode::FORBIDDEN,
        Json(AitError {
            message: "Admin privileges required".to_string(),
            code: 403,
            r#type: "forbidden".to_string(),
        }),
    )
}

pub fn forbidden_msg(msg: impl Into<String>) -> (StatusCode, Json<AitError>) {
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
            r#type: "auth_error".to_string(),
        }),
    )
}
