pub mod analytics;
pub mod apikeys;
pub mod logs;
pub mod models;
pub mod providers;
pub mod proxy;
pub mod stats;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn ident(c: char) -> bool {
        ident_chars(c)
    }

    #[test]
    fn empty_and_whitespace_rejected() {
        assert!(validate_string("", "name", 128, ident).is_err());
        assert!(validate_string("   ", "name", 128, ident).is_err());
    }

    #[test]
    fn too_long_rejected() {
        let long = "a".repeat(129);
        assert!(validate_string(&long, "name", 128, ident).is_err());
    }

    #[test]
    fn invalid_characters_rejected() {
        assert!(validate_string("bad#name", "name", 128, ident).is_err());
        assert!(validate_string("bad/name", "name", 128, ident).is_err());
    }

    #[test]
    fn valid_value_trimmed_and_accepted() {
        assert_eq!(
            validate_string("  my-name_1  ", "name", 128, ident).unwrap(),
            "my-name_1"
        );
        assert_eq!(
            validate_string("model-1:latest", "name", 128, model_name_chars).unwrap(),
            "model-1:latest"
        );
        assert_eq!(
            validate_string("abc-123", "id", 40, uuid_chars).unwrap(),
            "abc-123"
        );
        assert!(validate_string("abc@!$", "id", 40, uuid_chars).is_err());
    }
}
