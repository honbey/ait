//! Request body sensitive-data scanner.
//!
//! Scans JSON request bodies for configured literal sensitive values and
//! blocks requests that contain them. Matching is substring-based and
//! case-sensitive, operating only on JSON string values (not keys).

use crate::config::DlpConfig;
use serde_json::Value;

/// Scanner over the configured literal values.
///
/// A scanner is effectively disabled when no values are configured, even if
/// `enabled` is true, so an empty list never blocks anything.
#[derive(Clone)]
pub struct DlpScanner {
    enabled: bool,
    values: Vec<String>,
}

impl DlpScanner {
    pub fn new(config: &DlpConfig) -> Self {
        Self {
            enabled: config.enabled && !config.sensitive_values.is_empty(),
            values: config.sensitive_values.clone(),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Returns the first configured value found in the body, or `None` if no
    /// value matches. Only JSON string leaf values are inspected; object keys
    /// are ignored to avoid false matches on field names.
    pub fn scan(&self, body: &Value) -> Option<&str> {
        if !self.enabled {
            return None;
        }
        let text = collect_strings(body);
        self.values
            .iter()
            .find(|v| !v.is_empty() && text.contains(v.as_str()))
            .map(|v| v.as_str())
    }
}

fn collect_strings(value: &Value) -> String {
    let mut out = String::new();
    collect_strings_inner(value, &mut out);
    out
}

fn collect_strings_inner(value: &Value, out: &mut String) {
    match value {
        Value::String(s) => {
            out.push_str(s);
            out.push(' ');
        }
        Value::Array(items) => {
            for item in items {
                collect_strings_inner(item, out);
            }
        }
        Value::Object(map) => {
            for v in map.values() {
                collect_strings_inner(v, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(enabled: bool, values: &[&str]) -> DlpConfig {
        DlpConfig {
            enabled,
            sensitive_values: values.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn body(s: &str) -> Value {
        serde_json::json!({ "content": s })
    }

    #[test]
    fn disabled_scanner_matches_nothing() {
        let scanner = DlpScanner::new(&config(false, &["13800138000"]));
        assert!(scanner.scan(&body("my phone is 13800138000")).is_none());
    }

    #[test]
    fn empty_values_never_blocks() {
        let scanner = DlpScanner::new(&config(true, &[]));
        assert!(scanner.scan(&body("13800138000")).is_none());
        assert!(!scanner.is_enabled());
    }

    #[test]
    fn matches_substring_in_string_value() {
        let scanner = DlpScanner::new(&config(true, &["13800138000"]));
        assert_eq!(
            scanner.scan(&body("my phone is 13800138000")),
            Some("13800138000")
        );
    }

    #[test]
    fn does_not_match_embedded_in_different_value() {
        let scanner = DlpScanner::new(&config(true, &["13800138000"]));
        assert!(scanner.scan(&body("13800138001")).is_none());
    }

    #[test]
    fn case_sensitive_match() {
        let scanner = DlpScanner::new(&config(true, &["sk-secretvalue"]));
        assert!(scanner.scan(&body("SK-SECRETVALUE")).is_none());
        assert_eq!(
            scanner.scan(&body("sk-secretvalue")),
            Some("sk-secretvalue")
        );
    }

    #[test]
    fn ignores_object_keys() {
        let scanner = DlpScanner::new(&config(true, &["13800138000"]));
        let value = serde_json::json!({ "13800138000": "safe" });
        assert!(scanner.scan(&value).is_none());
    }

    #[test]
    fn scans_nested_arrays_and_objects() {
        let scanner = DlpScanner::new(&config(true, &["idcard"]));
        let value = serde_json::json!({
            "messages": [
                { "role": "user", "content": "here is idcard embedded" },
                { "role": "system", "content": "normal" }
            ]
        });
        assert_eq!(scanner.scan(&value), Some("idcard"));
    }

    #[test]
    fn returns_first_configured_rule_on_match() {
        let scanner = DlpScanner::new(&config(true, &["aaa", "bbb"]));
        // Returns the first configured rule that matches,
        // independent of where it appears in the body text.
        assert_eq!(scanner.scan(&body("bbb then aaa")), Some("aaa"));
        assert_eq!(scanner.scan(&body("aaa only")), Some("aaa"));
    }
}
