//! Rate limit header extraction, format conversion, and duration parsing.
//!
//! Converts between OpenAI-style `x-ratelimit-*` headers and Anthropic-style
//! `anthropic-ratelimit-*` headers. OpenAI uses relative durations ("1s", "500ms")
//! for reset fields; Anthropic uses ISO 8601 UTC timestamps.

use std::time::Duration;

/// Rate limit headers extracted from backend responses.
/// Forwarded to clients as Anthropic-style `anthropic-ratelimit-*` headers.
/// See: <https://docs.anthropic.com/en/api/rate-limits#response-headers>
#[derive(Debug, Default, Clone)]
pub struct RateLimitHeaders {
    /// Maximum requests allowed in the current window.
    pub requests_limit: Option<String>,
    /// Requests remaining before rate limiting kicks in.
    pub requests_remaining: Option<String>,
    /// Reset value for request limits (raw from backend).
    pub requests_reset: Option<String>,
    /// Maximum tokens allowed in the current window.
    pub tokens_limit: Option<String>,
    /// Tokens remaining before rate limiting kicks in.
    pub tokens_remaining: Option<String>,
    /// Reset value for token limits (raw from backend).
    pub tokens_reset: Option<String>,
    /// Seconds to wait before retrying (from `retry-after` header on 429s).
    pub retry_after: Option<String>,
    /// Anthropic organization ID from `anthropic-organization-id` response header.
    pub organization_id: Option<String>,
}

/// Extract a header value as a trimmed string.
fn header_str(headers: &reqwest::header::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
}

impl RateLimitHeaders {
    /// Extract rate limit headers from an OpenAI (or Vertex) response.
    pub fn from_openai_headers(headers: &reqwest::header::HeaderMap) -> Self {
        Self {
            requests_limit: header_str(headers, "x-ratelimit-limit-requests"),
            requests_remaining: header_str(headers, "x-ratelimit-remaining-requests"),
            requests_reset: header_str(headers, "x-ratelimit-reset-requests"),
            tokens_limit: header_str(headers, "x-ratelimit-limit-tokens"),
            tokens_remaining: header_str(headers, "x-ratelimit-remaining-tokens"),
            tokens_reset: header_str(headers, "x-ratelimit-reset-tokens"),
            retry_after: header_str(headers, "retry-after"),
            organization_id: None,
        }
    }

    /// Extract rate limit headers from an Anthropic response.
    /// Anthropic uses `anthropic-ratelimit-*` headers natively.
    pub fn from_anthropic_headers(headers: &reqwest::header::HeaderMap) -> Self {
        Self {
            requests_limit: header_str(headers, "anthropic-ratelimit-requests-limit"),
            requests_remaining: header_str(headers, "anthropic-ratelimit-requests-remaining"),
            requests_reset: header_str(headers, "anthropic-ratelimit-requests-reset"),
            tokens_limit: header_str(headers, "anthropic-ratelimit-tokens-limit"),
            tokens_remaining: header_str(headers, "anthropic-ratelimit-tokens-remaining"),
            tokens_reset: header_str(headers, "anthropic-ratelimit-tokens-reset"),
            retry_after: header_str(headers, "retry-after"),
            organization_id: header_str(headers, "anthropic-organization-id"),
        }
    }

    /// Inject Anthropic-format response headers (rate limits + version) into an
    /// `axum::http::HeaderMap`. The `*_reset` fields are converted from OpenAI's
    /// relative duration format (e.g., "1s") to Anthropic's ISO 8601 UTC timestamp.
    /// Falls back to the raw value with a warning if parsing fails.
    ///
    /// This method accepts generic `http::HeaderMap` (used by both reqwest and axum).
    pub fn inject_anthropic_response_headers(&self, map: &mut http_types::HeaderMap) {
        set_if_some(
            map,
            "anthropic-ratelimit-requests-limit",
            &self.requests_limit,
        );
        set_if_some(
            map,
            "anthropic-ratelimit-requests-remaining",
            &self.requests_remaining,
        );
        let req_reset = convert_reset_duration(&self.requests_reset, "requests_reset");
        set_if_some(map, "anthropic-ratelimit-requests-reset", &req_reset);
        set_if_some(map, "anthropic-ratelimit-tokens-limit", &self.tokens_limit);
        set_if_some(
            map,
            "anthropic-ratelimit-tokens-remaining",
            &self.tokens_remaining,
        );
        let tok_reset = convert_reset_duration(&self.tokens_reset, "tokens_reset");
        set_if_some(map, "anthropic-ratelimit-tokens-reset", &tok_reset);
        set_if_some(map, "retry-after", &self.retry_after);
        set_if_some(map, "anthropic-organization-id", &self.organization_id);
        map.insert(
            http_types::HeaderName::from_static("anthropic-version"),
            http_types::HeaderValue::from_static("2023-06-01"),
        );
    }
}

// Use the http crate types that reqwest re-exports, avoiding an extra dependency.
mod http_types {
    pub use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
}

/// Set a header on a HeaderMap if the value is Some.
fn set_if_some(map: &mut http_types::HeaderMap, name: &str, value: &Option<String>) {
    if let Some(v) = value {
        if let (Ok(header_name), Ok(header_value)) = (
            http_types::HeaderName::from_bytes(name.as_bytes()),
            http_types::HeaderValue::from_str(v),
        ) {
            map.insert(header_name, header_value);
        }
    }
}

/// Convert an OpenAI relative duration to ISO 8601, falling back to the raw
/// value with a warning if parsing fails.
fn convert_reset_duration(raw: &Option<String>, field: &str) -> Option<String> {
    raw.as_deref().map(|v| {
        openai_duration_to_iso8601(v).unwrap_or_else(|| {
            tracing::warn!(
                value = v,
                field,
                "failed to parse reset duration, forwarding raw"
            );
            v.to_string()
        })
    })
}

/// Convert an OpenAI relative duration string to an ISO 8601 UTC timestamp.
fn openai_duration_to_iso8601(s: &str) -> Option<String> {
    openai_duration_to_iso8601_at(s, std::time::SystemTime::now())
}

/// Convert an OpenAI relative duration string to an ISO 8601 UTC timestamp
/// by adding it to the given anchor time. Testable variant.
pub fn openai_duration_to_iso8601_at(s: &str, anchor: std::time::SystemTime) -> Option<String> {
    let dur = parse_openai_duration(s)?;
    let reset_time = anchor + dur;
    let secs = reset_time
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(epoch_to_iso8601(secs))
}

/// Parse OpenAI's duration format (e.g., "6ms", "1s", "1m30s", "2m") into a
/// [`Duration`]. Returns `None` for unrecognized formats.
pub fn parse_openai_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut total_ms: u64 = 0;
    let mut num_start: Option<usize> = None;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_digit() || c == b'.' {
            if num_start.is_none() {
                num_start = Some(i);
            }
            i += 1;
        } else if c.is_ascii_alphabetic() {
            let start = num_start?;
            let num_str = &s[start..i];
            let unit_start = i;
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            let unit = &s[unit_start..i];
            let value: f64 = num_str.parse().ok()?;
            let ms = match unit {
                "ms" => value,
                "s" => value * 1_000.0,
                "m" => value * 60_000.0,
                "h" => value * 3_600_000.0,
                _ => return None,
            };
            total_ms += ms.round() as u64;
            num_start = None;
        } else {
            return None;
        }
    }
    // Trailing number with no unit is invalid
    if num_start.is_some() {
        return None;
    }
    Some(Duration::from_millis(total_ms))
}

/// Convert epoch seconds to ISO 8601 UTC string (e.g., "2025-06-16T12:00:01Z").
pub fn epoch_to_iso8601(epoch: u64) -> String {
    let secs = epoch;
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let (year, month, day) = days_to_ymd(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Days since 1970-01-01 to (year, month, day).
/// Algorithm from <http://howardhinnant.github.io/date_algorithms.html>
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_openai_headers_extracts_all() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("x-ratelimit-limit-requests", "100".parse().unwrap());
        headers.insert("x-ratelimit-remaining-requests", "99".parse().unwrap());
        headers.insert("x-ratelimit-reset-requests", "1s".parse().unwrap());
        headers.insert("x-ratelimit-limit-tokens", "40000".parse().unwrap());
        headers.insert("x-ratelimit-remaining-tokens", "39500".parse().unwrap());
        headers.insert("x-ratelimit-reset-tokens", "500ms".parse().unwrap());
        headers.insert("retry-after", "2".parse().unwrap());

        let rl = RateLimitHeaders::from_openai_headers(&headers);
        assert_eq!(rl.requests_limit.as_deref(), Some("100"));
        assert_eq!(rl.requests_remaining.as_deref(), Some("99"));
        assert_eq!(rl.requests_reset.as_deref(), Some("1s"));
        assert_eq!(rl.tokens_limit.as_deref(), Some("40000"));
        assert_eq!(rl.tokens_remaining.as_deref(), Some("39500"));
        assert_eq!(rl.tokens_reset.as_deref(), Some("500ms"));
        assert_eq!(rl.retry_after.as_deref(), Some("2"));
    }

    #[test]
    fn from_openai_headers_missing_are_none() {
        let headers = reqwest::header::HeaderMap::new();
        let rl = RateLimitHeaders::from_openai_headers(&headers);
        assert!(rl.requests_limit.is_none());
        assert!(rl.requests_remaining.is_none());
        assert!(rl.requests_reset.is_none());
        assert!(rl.tokens_limit.is_none());
        assert!(rl.tokens_remaining.is_none());
        assert!(rl.tokens_reset.is_none());
        assert!(rl.retry_after.is_none());
    }

    #[test]
    fn inject_anthropic_response_headers_sets_values() {
        let rl = RateLimitHeaders {
            requests_limit: Some("100".into()),
            tokens_remaining: Some("39500".into()),
            retry_after: Some("3".into()),
            ..Default::default()
        };
        let mut map = reqwest::header::HeaderMap::new();
        rl.inject_anthropic_response_headers(&mut map);

        assert_eq!(
            map.get("anthropic-ratelimit-requests-limit").unwrap(),
            "100"
        );
        assert_eq!(
            map.get("anthropic-ratelimit-tokens-remaining").unwrap(),
            "39500"
        );
        assert_eq!(map.get("retry-after").unwrap(), "3");
        assert_eq!(map.get("anthropic-version").unwrap(), "2023-06-01");
        assert!(map.get("anthropic-ratelimit-requests-remaining").is_none());
        assert!(map.get("anthropic-ratelimit-tokens-limit").is_none());
    }

    #[test]
    fn inject_anthropic_response_headers_default_sets_version_only() {
        let rl = RateLimitHeaders::default();
        let mut map = reqwest::header::HeaderMap::new();
        rl.inject_anthropic_response_headers(&mut map);
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("anthropic-version").unwrap(), "2023-06-01");
    }

    #[test]
    fn inject_anthropic_response_headers_converts_reset_to_iso8601() {
        let rl = RateLimitHeaders {
            requests_reset: Some("1s".into()),
            tokens_reset: Some("500ms".into()),
            ..Default::default()
        };
        let mut map = reqwest::header::HeaderMap::new();
        rl.inject_anthropic_response_headers(&mut map);

        let req_reset = map
            .get("anthropic-ratelimit-requests-reset")
            .unwrap()
            .to_str()
            .unwrap();
        let tok_reset = map
            .get("anthropic-ratelimit-tokens-reset")
            .unwrap()
            .to_str()
            .unwrap();
        // inject_anthropic_response_headers uses SystemTime::now() internally so we
        // can only check the format here, not the exact value. The formatter itself
        // is tested end-to-end with exact expected values in
        // openai_duration_to_iso8601_at_pinned_time below.
        assert!(
            req_reset.contains('T') && req_reset.ends_with('Z'),
            "expected ISO 8601 timestamp, got: {req_reset}"
        );
        assert!(
            tok_reset.contains('T') && tok_reset.ends_with('Z'),
            "expected ISO 8601 timestamp, got: {tok_reset}"
        );
    }

    #[test]
    fn parse_openai_duration_various_formats() {
        assert_eq!(parse_openai_duration("6ms"), Some(Duration::from_millis(6)));
        assert_eq!(
            parse_openai_duration("1s"),
            Some(Duration::from_millis(1000))
        );
        assert_eq!(
            parse_openai_duration("2m"),
            Some(Duration::from_millis(120_000))
        );
        assert_eq!(
            parse_openai_duration("1m30s"),
            Some(Duration::from_millis(90_000))
        );
        assert_eq!(
            parse_openai_duration("1h"),
            Some(Duration::from_millis(3_600_000))
        );
        assert_eq!(
            parse_openai_duration("1h30m"),
            Some(Duration::from_millis(5_400_000))
        );
    }

    #[test]
    fn parse_openai_duration_invalid() {
        assert_eq!(parse_openai_duration(""), None);
        assert_eq!(parse_openai_duration("abc"), None);
        assert_eq!(parse_openai_duration("123"), None);
        assert_eq!(parse_openai_duration("1x"), None);
    }

    #[test]
    fn openai_duration_to_iso8601_at_pinned_time() {
        let anchor = std::time::UNIX_EPOCH + Duration::from_secs(1_750_075_200);
        assert_eq!(
            openai_duration_to_iso8601_at("1s", anchor).unwrap(),
            "2025-06-16T12:00:01Z"
        );
        assert_eq!(
            openai_duration_to_iso8601_at("1m30s", anchor).unwrap(),
            "2025-06-16T12:01:30Z"
        );
        assert_eq!(
            openai_duration_to_iso8601_at("500ms", anchor).unwrap(),
            "2025-06-16T12:00:00Z"
        );
    }

    #[test]
    fn openai_duration_to_iso8601_invalid_returns_none() {
        assert!(openai_duration_to_iso8601("garbage").is_none());
        assert!(openai_duration_to_iso8601("").is_none());
    }

    #[test]
    fn epoch_to_iso8601_unix_epoch() {
        assert_eq!(epoch_to_iso8601(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn from_anthropic_headers_extracts_all() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("anthropic-ratelimit-requests-limit", "100".parse().unwrap());
        headers.insert(
            "anthropic-ratelimit-requests-remaining",
            "99".parse().unwrap(),
        );
        headers.insert(
            "anthropic-ratelimit-requests-reset",
            "2025-01-01T00:00:00Z".parse().unwrap(),
        );
        headers.insert("retry-after", "5".parse().unwrap());

        let rl = RateLimitHeaders::from_anthropic_headers(&headers);
        assert_eq!(rl.requests_limit.as_deref(), Some("100"));
        assert_eq!(rl.requests_remaining.as_deref(), Some("99"));
        assert_eq!(rl.requests_reset.as_deref(), Some("2025-01-01T00:00:00Z"));
        assert_eq!(rl.retry_after.as_deref(), Some("5"));
        assert!(rl.organization_id.is_none());
    }

    #[test]
    fn from_anthropic_headers_parses_organization_id() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("anthropic-organization-id", "org-abc123".parse().unwrap());
        let rl = RateLimitHeaders::from_anthropic_headers(&headers);
        assert_eq!(rl.organization_id.as_deref(), Some("org-abc123"));
    }

    #[test]
    fn inject_anthropic_response_headers_sets_organization_id() {
        let rl = RateLimitHeaders {
            organization_id: Some("org-xyz".into()),
            ..Default::default()
        };
        let mut map = reqwest::header::HeaderMap::new();
        rl.inject_anthropic_response_headers(&mut map);
        assert_eq!(map.get("anthropic-organization-id").unwrap(), "org-xyz");
        assert_eq!(map.get("anthropic-version").unwrap(), "2023-06-01");
    }
}
