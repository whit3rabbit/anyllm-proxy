//! Generic retry logic with exponential backoff and jitter.

use reqwest::Client;
use serde::Serialize;
use std::time::Duration;
use tokio::time::sleep;

/// Default maximum number of retries.
pub const MAX_RETRIES: u32 = 3;

/// Default base delay between retries in milliseconds.
pub const BASE_DELAY_MS: u64 = 500;

/// Backend error types implement this to enable the generic [`send_with_retry`].
pub trait RetryableError: Sized {
    fn from_request(e: reqwest::Error) -> Self;
    fn from_api_response(status: u16, body: &str) -> Self;
}

/// Authentication to apply to outgoing requests.
#[derive(Clone, Debug)]
pub enum RequestAuth<'a> {
    Bearer(&'a str),
    Header { name: &'a str, value: &'a str },
}

fn apply_auth(rb: reqwest::RequestBuilder, auth: &RequestAuth<'_>) -> reqwest::RequestBuilder {
    match auth {
        RequestAuth::Bearer(token) => rb.bearer_auth(token),
        RequestAuth::Header { name, value } => rb.header(*name, *value),
    }
}

/// Send a POST request with retry on 429/5xx. Returns the raw successful response.
pub async fn send_with_retry<E: RetryableError>(
    client: &Client,
    url: &str,
    auth: &RequestAuth<'_>,
    body: &impl Serialize,
    label: &str,
    max_retries: u32,
) -> Result<reqwest::Response, E> {
    for attempt in 0..=max_retries {
        let rb = apply_auth(client.post(url).json(body), auth);
        let response = rb.send().await.map_err(E::from_request)?;
        let status = response.status().as_u16();

        if (200..300).contains(&status) {
            return Ok(response);
        }

        if attempt < max_retries && is_retryable(status) {
            let retry_after = parse_retry_after(response.headers());
            let delay = backoff_delay(attempt, retry_after);
            tracing::warn!(
                status,
                attempt = attempt + 1,
                max_retries,
                delay_ms = delay.as_millis() as u64,
                "retryable error from {label}, backing off"
            );
            // Drain the response body before retrying so the HTTP connection
            // returns to the pool. Leaving it unread causes connection leaks.
            drop(response.bytes().await);
            sleep(delay).await;
            continue;
        }

        let text = response.text().await.unwrap_or_else(|e| {
            tracing::warn!("failed to read error response body: {e}");
            String::new()
        });
        return Err(E::from_api_response(status, &text));
    }

    unreachable!("loop runs max_retries+1 times and always returns")
}

/// Check if a status code is retryable (408, 429, or 5xx).
pub fn is_retryable(status: u16) -> bool {
    status == 408 || status == 429 || (500..=599).contains(&status)
}

/// Parse retry-after header as integer seconds or HTTP date (RFC 7231).
pub fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let value = headers
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())?;
    // Integer seconds (most common case).
    if let Ok(secs) = value.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    // Fractional seconds (e.g. "1.5" from some backends).
    if let Ok(secs) = value.parse::<f64>() {
        return Some(Duration::from_secs_f64(secs.max(0.0)));
    }
    // HTTP date (RFC 7231). Past dates return None (no wait needed).
    let date = httpdate::parse_http_date(&value).ok()?;
    date.duration_since(std::time::SystemTime::now()).ok()
}

/// Compute backoff delay with jitter.
///
/// Uses deterministic 25% jitter (upper bound, not random) to keep tests
/// predictable while still spreading retry storms across backends.
pub fn backoff_delay(attempt: u32, retry_after: Option<Duration>) -> Duration {
    if let Some(ra) = retry_after {
        return ra;
    }
    let base = Duration::from_millis(BASE_DELAY_MS * 2u64.pow(attempt));
    let jitter_ms = (base.as_millis() as u64) / 4;
    base + Duration::from_millis(jitter_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_retryable_429() {
        assert!(is_retryable(429));
    }

    #[test]
    fn is_retryable_5xx() {
        assert!(is_retryable(500));
        assert!(is_retryable(502));
        assert!(is_retryable(503));
        assert!(is_retryable(599));
    }

    #[test]
    fn is_retryable_408() {
        assert!(is_retryable(408));
    }

    #[test]
    fn is_not_retryable_4xx() {
        assert!(!is_retryable(400));
        assert!(!is_retryable(401));
        assert!(!is_retryable(404));
        assert!(!is_retryable(409));
    }

    #[test]
    fn backoff_respects_retry_after() {
        let delay = backoff_delay(0, Some(Duration::from_secs(5)));
        assert_eq!(delay, Duration::from_secs(5));
    }

    #[test]
    fn backoff_increases_with_attempt() {
        let d0 = backoff_delay(0, None);
        let d1 = backoff_delay(1, None);
        let d2 = backoff_delay(2, None);
        assert!(d1 > d0);
        assert!(d2 > d1);
    }

    #[test]
    fn parse_retry_after_valid() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("retry-after", "3".parse().unwrap());
        let dur = parse_retry_after(&headers);
        assert_eq!(dur, Some(Duration::from_secs(3)));
    }

    #[test]
    fn parse_retry_after_missing() {
        let headers = reqwest::header::HeaderMap::new();
        assert_eq!(parse_retry_after(&headers), None);
    }

    #[test]
    fn parse_retry_after_http_date_future() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "retry-after",
            "Wed, 21 Oct 2037 07:28:00 GMT".parse().unwrap(),
        );
        let dur = parse_retry_after(&headers);
        assert!(dur.is_some(), "future HTTP date should parse to Some");
        assert!(dur.unwrap().as_secs() > 0);
    }

    #[test]
    fn parse_retry_after_http_date_past() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "retry-after",
            "Mon, 01 Jan 2024 00:00:00 GMT".parse().unwrap(),
        );
        assert_eq!(parse_retry_after(&headers), None);
    }

    #[test]
    fn parse_retry_after_fractional_seconds() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("retry-after", "1.5".parse().unwrap());
        let dur = parse_retry_after(&headers);
        assert_eq!(dur, Some(Duration::from_secs_f64(1.5)));
    }

    #[test]
    fn parse_retry_after_garbage() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("retry-after", "not-a-date-or-number".parse().unwrap());
        assert_eq!(parse_retry_after(&headers), None);
    }
}
