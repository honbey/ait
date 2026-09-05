//! Request body sensitive-data scanner.
//!
//! Scans JSON request bodies for configured literal sensitive values and
//! blocks requests that contain them. Matching is substring-based and
//! case-sensitive, operating only on JSON string values (not keys).
//!
//! All rules feed a single Aho-Corasick automaton, so each string leaf is
//! scanned in one pass regardless of how many rules are configured.

use crate::config::DlpConfig;
use aho_corasick::AhoCorasick;
use serde_json::Value;

/// Scanner over the configured literal values.
///
/// A scanner is effectively disabled when no values are configured, even if
/// `enabled` is true, so an empty list never blocks anything.
#[derive(Clone)]
pub struct DlpScanner {
    enabled: bool,
    /// Non-empty configured rules, in config order (index = priority).
    rules: Vec<String>,
    /// Multi-pattern matcher over `rules`; `None` if the automaton failed to
    /// build, in which case detection is disabled.
    matcher: Option<AhoCorasick>,
    /// Whether JSON number literals are scanned; see
    /// [`crate::config::DlpConfig::scan_numbers`].
    scan_numbers: bool,
}

impl DlpScanner {
    pub fn new(config: &DlpConfig) -> Self {
        // Empty rules would match everywhere; drop them up front.
        let rules: Vec<String> = config
            .sensitive_values
            .iter()
            .filter(|v| !v.is_empty())
            .cloned()
            .collect();
        let enabled = config.enabled && !rules.is_empty();
        let matcher = if enabled {
            match AhoCorasick::new(rules.iter().map(String::as_str)) {
                Ok(matcher) => Some(matcher),
                Err(e) => {
                    tracing::warn!("[dlp] automaton build failed, detection disabled: {e}");
                    None
                }
            }
        } else {
            None
        };
        Self {
            enabled: enabled && matcher.is_some(),
            rules,
            matcher,
            scan_numbers: config.scan_numbers,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Returns the first configured value found in the body, or `None` if no
    /// value matches. JSON string leaves are always inspected; number leaves
    /// only when `scan_numbers` is enabled. Object keys are never inspected,
    /// to avoid false matches on field names.
    pub fn scan(&self, body: &Value) -> Option<&str> {
        if !self.enabled {
            return None;
        }
        let Some(matcher) = &self.matcher else {
            return None;
        };
        // Index of the earliest configured rule matching any scanned leaf.
        // Scanning leaf-by-leaf keeps cross-leaf matches impossible (the
        // previous concatenated buffer separated leaves with a space).
        let mut best: Option<usize> = None;
        scan_value(body, matcher, self.scan_numbers, &mut best);
        best.map(|idx| self.rules[idx].as_str())
    }
}

fn scan_value(value: &Value, matcher: &AhoCorasick, scan_numbers: bool, best: &mut Option<usize>) {
    if *best == Some(0) {
        return; // cannot improve on the first configured rule
    }
    match value {
        Value::String(s) => scan_text(s, matcher, best),
        // A rule that is purely numeric can arrive as a JSON number literal,
        // which a string-only scan never sees.
        Value::Number(n) if scan_numbers => scan_text(&n.to_string(), matcher, best),
        Value::Array(items) => {
            for item in items {
                scan_value(item, matcher, scan_numbers, best);
                if *best == Some(0) {
                    return;
                }
            }
        }
        Value::Object(map) => {
            for v in map.values() {
                scan_value(v, matcher, scan_numbers, best);
                if *best == Some(0) {
                    return;
                }
            }
        }
        _ => {}
    }
}

/// Record the earliest configured rule occurring in `text`.
fn scan_text(text: &str, matcher: &AhoCorasick, best: &mut Option<usize>) {
    for mat in matcher.find_iter(text) {
        let idx = mat.pattern().as_usize();
        *best = Some(match *best {
            Some(prev) => prev.min(idx),
            None => idx,
        });
        if *best == Some(0) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(enabled: bool, values: &[&str]) -> DlpConfig {
        DlpConfig {
            enabled,
            sensitive_values: values.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    /// Same as [`config`] with number-literal scanning turned on.
    fn config_scanning_numbers(enabled: bool, values: &[&str]) -> DlpConfig {
        DlpConfig {
            scan_numbers: true,
            ..config(enabled, values)
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
    fn number_leaf_ignored_by_default() {
        let scanner = DlpScanner::new(&config(true, &["13800138000"]));
        // A purely numeric rule arriving as a JSON number literal stays
        // invisible unless scan_numbers is enabled.
        assert!(
            scanner
                .scan(&serde_json::json!({"user_id": 13800138000u64}))
                .is_none()
        );
        // The same digits inside a string are still caught.
        assert_eq!(
            scanner.scan(&body("my phone is 13800138000")),
            Some("13800138000")
        );
    }

    #[test]
    fn number_leaf_matched_when_scan_numbers_enabled() {
        let scanner = DlpScanner::new(&config_scanning_numbers(true, &["13800138000"]));
        assert_eq!(
            scanner.scan(&serde_json::json!({"user_id": 13800138000u64})),
            Some("13800138000")
        );
        // Nested numbers are reached the same way.
        assert_eq!(
            scanner.scan(&serde_json::json!({"a": [{"b": 13800138000u64}]})),
            Some("13800138000")
        );
    }

    #[test]
    fn scan_numbers_does_not_change_other_behaviour() {
        let scanner = DlpScanner::new(&config_scanning_numbers(true, &["13800138000"]));
        // Strings still match...
        assert_eq!(
            scanner.scan(&body("my phone is 13800138000")),
            Some("13800138000")
        );
        // ...a different number does not...
        assert!(
            scanner
                .scan(&serde_json::json!({"user_id": 13800138001u64}))
                .is_none()
        );
        // ...and object keys stay out of scope.
        assert!(
            scanner
                .scan(&serde_json::json!({ "13800138000": "safe" }))
                .is_none()
        );
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
