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
