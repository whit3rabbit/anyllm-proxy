// Secret redaction for safe logging. Avoids leaking full API keys
// into log output while keeping enough to identify the key.

/// Redact a secret string for logging, showing only the first 4 and last 4 characters.
/// Returns "****" for strings shorter than 12 characters.
pub fn redact_secret(s: &str) -> String {
    if s.len() < 12 {
        "****".to_string()
    } else {
        format!("{}...{}", &s[..4], &s[s.len() - 4..])
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
}
