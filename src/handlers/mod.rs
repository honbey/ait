pub mod analytics;
pub mod apikeys;
pub mod auth;
pub mod logs;
pub mod models;
pub mod providers;
pub mod proxy;
pub mod stats;
pub mod users;

use crate::error::AitError;

pub(crate) fn validate_string(
    value: &str,
    field_name: &str,
    max: usize,
    allowed_chars: fn(char) -> bool,
) -> Result<String, AitError> {
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        return Err(AitError::bad_request(format!(
            "{} must not be empty",
            field_name
        )));
    }
    if trimmed.len() > max {
        return Err(AitError::bad_request(format!(
            "{} must not exceed {} characters",
            field_name, max
        )));
    }
    if !trimmed.chars().all(allowed_chars) {
        return Err(AitError::bad_request(format!(
            "{} contains invalid characters",
            field_name
        )));
    }
    Ok(trimmed)
}

pub(crate) fn ident_chars(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.')
}

pub(crate) fn model_name_chars(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.' | ':')
}

pub(crate) fn upstream_model_chars(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.' | ':' | '/' | '(' | ')' | '@')
}

pub(crate) fn uuid_chars(c: char) -> bool {
    c.is_ascii_hexdigit() || c == '-'
}
