/// Mask a sensitive string by keeping `prefix_len` leading chars and
/// `suffix_len` trailing chars, replacing the middle with `******`.
///
/// If the value is too short to show both parts without overlap, the whole
/// value is replaced with `******`.
pub fn mask_sensitive_value(value: &str, prefix_len: usize, suffix_len: usize) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= prefix_len + suffix_len {
        "******".to_string()
    } else {
        let prefix: String = chars[..prefix_len].iter().collect();
        let suffix: String = chars[chars.len() - suffix_len..].iter().collect();
        format!("{}******{}", prefix, suffix)
    }
}

/// Mask an API key: keep the first 6 and last 3 chars.
pub fn mask_api_key(key: &str) -> String {
    mask_sensitive_value(key, 6, 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_ascii_key() {
        assert_eq!(mask_api_key("sk-abc1234567890xyz"), "sk-abc******xyz");
    }

    #[test]
    fn mask_multibyte_key_does_not_panic() {
        // "密钥" x6 is 12 chars: first 6 + mask + last 3
        let key = "密钥密钥密钥密钥密钥密钥";
        let masked = mask_api_key(key);
        assert_eq!(masked, "密钥密钥密钥******钥密钥");
    }

    #[test]
    fn mask_short_key() {
        assert_eq!(mask_api_key("sk-abcd"), "******");
    }

    #[test]
    fn mask_empty_key() {
        assert_eq!(mask_api_key(""), "******");
    }

    #[test]
    fn mask_sensitive_value_prefix_suffix() {
        assert_eq!(mask_sensitive_value("13800138000", 3, 2), "138******00");
    }

    #[test]
    fn mask_sensitive_value_too_short() {
        assert_eq!(mask_sensitive_value("abc", 3, 2), "******");
    }

    #[test]
    fn mask_sensitive_value_zero_parts() {
        assert_eq!(mask_sensitive_value("abcdef", 0, 0), "******");
    }
}
