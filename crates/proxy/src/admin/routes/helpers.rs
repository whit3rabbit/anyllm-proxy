use std::sync::atomic::Ordering;

/// Validate that a string looks like an ISO 8601 / RFC 3339 timestamp.
/// Accepts YYYY-MM-DD (date only) or YYYY-MM-DDTHH:MM:SS[...] (datetime).
/// Does not check calendar validity — the goal is to reject strings that
/// would bypass the timestamp index and force a full-table scan.
pub fn is_valid_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 10 {
        return false;
    }
    b[0..4].iter().all(|c| c.is_ascii_digit())
        && b[4] == b'-'
        && b[5..7].iter().all(|c| c.is_ascii_digit())
        && b[7] == b'-'
        && b[8..10].iter().all(|c| c.is_ascii_digit())
        && (b.len() == 10
            || (b.len() >= 19
                && (b[10] == b'T' || b[10] == b' ')
                && b[11..13].iter().all(|c| c.is_ascii_digit())
                && b[13] == b':'
                && b[14..16].iter().all(|c| c.is_ascii_digit())
                && b[16] == b':'
                && b[17..19].iter().all(|c| c.is_ascii_digit())))
}

/// Returns the name of the first `since`/`until` parameter that fails timestamp
/// validation, or `None` if both are valid (or absent).
pub fn check_time_range(since: Option<&str>, until: Option<&str>) -> Option<&'static str> {
    if since.is_some_and(|s| !is_valid_timestamp(s)) {
        return Some("since");
    }
    if until.is_some_and(|u| !is_valid_timestamp(u)) {
        return Some("until");
    }
    None
}

/// Reject model names containing path traversal sequences or suspicious characters.
/// Only alphanumerics plus `-_./: @` are allowed (covers known provider naming
/// conventions like `gpt-4o`, `us.meta.llama3-2-1b-instruct-v1:0`,
/// `accounts/fireworks/models/llama-v3p1-8b-instruct`).
pub fn is_safe_model_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("..")
        && !name.contains('?')
        && !name.contains('#')
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || "-_./:@".contains(c))
}

/// Base64url-encode without padding (RFC 4648 section 5).
pub fn base64_url_encode(input: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(input)
}

/// Override both admin rate limits (requests per minute per IP).
/// Intended for integration tests that need a higher limit.
pub fn set_admin_rpm(rpm: u32) {
    super::middleware::ADMIN_READ_RPM.store(rpm, Ordering::Relaxed);
    super::middleware::ADMIN_WRITE_RPM.store(rpm, Ordering::Relaxed);
}

/// Clear all rate limit state. Exposed for integration tests.
pub fn reset_admin_rate_limit() {
    super::middleware::READ_RATE_BUCKETS.clear();
    super::middleware::WRITE_RATE_BUCKETS.clear();
}
