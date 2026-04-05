// Secret redaction for safe logging. Avoids leaking full API keys
// into log output while keeping enough to identify the key.

/// Redact a secret string for logging, showing only the first 4 and last 4 characters.
/// Returns "****" for strings shorter than 12 characters (too short to redact safely).
/// Uses char_indices() instead of byte offsets because API keys may contain
/// multi-byte UTF-8 characters; byte slicing would panic at non-char boundaries.
///
/// # Note
/// Redacted values are for display/logging only and may collide: two different
/// keys with identical first and last 4 characters produce the same redacted string.
/// Never use redacted values as cache keys, metric labels, or unique identifiers.
pub fn redact_secret(s: &str) -> String {
    let char_count = s.chars().count();
    if char_count < 12 {
        "****".to_string()
    } else {
        // Find byte offset of the 4th char boundary for prefix.
        let prefix_end = s.char_indices().nth(4).map(|(i, _)| i).unwrap_or(s.len());
        // Find byte offset of the (len-4)th char boundary for suffix.
        let suffix_start = s
            .char_indices()
            .nth(char_count - 4)
            .map(|(i, _)| i)
            .unwrap_or(0);
        format!("{}...{}", &s[..prefix_end], &s[suffix_start..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string() {
        assert_eq!(redact_secret(""), "****");
    }

    #[test]
    fn short_string() {
        assert_eq!(redact_secret("abc"), "****");
    }

    #[test]
    fn exactly_11_chars() {
        assert_eq!(redact_secret("12345678901"), "****");
    }

    #[test]
    fn exactly_12_chars() {
        assert_eq!(redact_secret("123456789012"), "1234...9012");
    }

    #[test]
    fn typical_api_key() {
        let key = "sk-proj-abcdefghijklmnop";
        let redacted = redact_secret(key);
        assert_eq!(redacted, "sk-p...mnop");
    }

    #[test]
    fn multibyte_utf8_does_not_panic() {
        // 12 chars, but 24 bytes (each char is 2 bytes in UTF-8).
        let key = "\u{00e9}\u{00e9}\u{00e9}\u{00e9}\u{00e9}\u{00e9}\u{00e9}\u{00e9}\u{00e9}\u{00e9}\u{00e9}\u{00e9}";
        assert_eq!(key.chars().count(), 12);
        let redacted = redact_secret(key);
        assert_eq!(
            redacted,
            "\u{00e9}\u{00e9}\u{00e9}\u{00e9}...\u{00e9}\u{00e9}\u{00e9}\u{00e9}"
        );
    }

    #[test]
    fn multibyte_short_string() {
        // 6 chars but 12 bytes -- should still be "****" because char count < 12.
        let key = "\u{00e9}\u{00e9}\u{00e9}\u{00e9}\u{00e9}\u{00e9}";
        assert_eq!(key.chars().count(), 6);
        assert_eq!(redact_secret(key), "****");
    }
}
