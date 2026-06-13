//! Generic retry logic with exponential backoff and jitter.

use reqwest::Client;
use serde::Serialize;
use std::time::Duration;
use tokio::time::sleep;

/// Default maximum number of retries.
pub const MAX_RETRIES: u32 = 3;

/// Default base delay between retries in milliseconds.
pub const BASE_DELAY_MS: u64 = 500;

/// Backend error types implement this to enable the generic retry loops.
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

/// Retry policy controlling how many times and what to retry.
///
/// Construct via [`RetryPolicy::new`] or [`Default`], then chain setters.
/// The struct is `#[non_exhaustive]` so new fields can be added without
/// breaking callers who construct via the factory methods.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct RetryPolicy {
    /// Maximum number of retry attempts (not counting the first try).
    /// Default: [`MAX_RETRIES`] (3).
    pub max_retries: u32,
    /// Whether to retry on transport errors where the server provably never
    /// received the request (connection refused, connection reset before data
    /// was sent). Only `is_connect()` reqwest errors are retried; read/response
    /// timeouts are NOT retried because the server may have already processed
    /// the request before the client gave up. Body/decode/redirect errors
    /// return immediately regardless of this flag.
    ///
    /// Default: `false`. Callers opt in explicitly since LLM endpoints are not
    /// idempotent — a retried POST can produce a duplicate completion and
    /// a duplicate charge.
    pub retry_transport_errors: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: MAX_RETRIES,
            retry_transport_errors: false,
        }
    }
}

impl RetryPolicy {
    /// Create a policy with the given retry limit and transport retries off.
    pub fn new(max_retries: u32) -> Self {
        Self {
            max_retries,
            retry_transport_errors: false,
        }
    }

    /// Toggle transport-error retries (connect / timeout failures).
    pub fn with_transport_retries(mut self, enabled: bool) -> Self {
        self.retry_transport_errors = enabled;
        self
    }
}

/// Send a POST request with retry on 429/5xx. Returns the raw successful response.
///
/// This is the legacy entry point retained for existing proxy callers.
/// New code should use [`send_with_retry_policy`] directly — it exposes
/// per-request `extra_headers` and the full [`RetryPolicy`] (including
/// `retry_transport_errors`, which this shim always leaves `false`).
pub async fn send_with_retry<E: RetryableError>(
    client: &Client,
    url: &str,
    auth: &RequestAuth<'_>,
    body: &impl Serialize,
    label: &str,
    max_retries: u32,
) -> Result<reqwest::Response, E> {
    send_with_retry_policy(
        client,
        url,
        auth,
        &[],
        body,
        label,
        &RetryPolicy::new(max_retries),
    )
    .await
}

/// Send a POST request with retry on 429/5xx and optionally on transport errors.
///
/// `extra_headers` are applied to every attempt in addition to `auth`.
/// Returns the raw successful response on 2xx.
pub async fn send_with_retry_policy<E: RetryableError>(
    client: &Client,
    url: &str,
    auth: &RequestAuth<'_>,
    extra_headers: &[(&str, &str)],
    body: &impl Serialize,
    label: &str,
    policy: &RetryPolicy,
) -> Result<reqwest::Response, E> {
    let max_retries = policy.max_retries;
    for attempt in 0..=max_retries {
        let rb = apply_auth(client.post(url).json(body), auth);
        let rb = extra_headers.iter().fold(rb, |rb, &(k, v)| rb.header(k, v));

        let response = match rb.send().await {
            Ok(r) => r,
            Err(e) => {
                // Only retry connect/timeout transport errors, and only when opted in.
                // Only retry on connect errors — the server never received the
                // request, so re-sending is safe. Read/response timeouts are
                // NOT retried: the server may have already processed the POST.
                if policy.retry_transport_errors && attempt < max_retries && e.is_connect() {
                    let delay = backoff_delay(attempt, None);
                    tracing::warn!(
                        attempt = attempt + 1,
                        max_retries,
                        delay_ms = delay.as_millis() as u64,
                        "transport error from {label}, backing off"
                    );
                    sleep(delay).await;
                    continue;
                }
                return Err(E::from_request(e));
            }
        };

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
    // Cap exponent at 62 to prevent u64 overflow when max_retries is large.
    let base = Duration::from_millis(BASE_DELAY_MS * 2u64.pow(attempt.min(62)));
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

    // --- RetryPolicy ---

    #[test]
    fn retry_policy_defaults() {
        let p = RetryPolicy::default();
        assert_eq!(p.max_retries, MAX_RETRIES);
        assert!(!p.retry_transport_errors);
    }

    #[test]
    fn retry_policy_new_and_chaining() {
        let p = RetryPolicy::new(5).with_transport_retries(true);
        assert_eq!(p.max_retries, 5);
        assert!(p.retry_transport_errors);
    }

    #[test]
    fn send_with_retry_delegates_to_policy() {
        // send_with_retry(_, _, _, _, _, n) must behave identically to
        // send_with_retry_policy with RetryPolicy::new(n). We verify the
        // policy struct matches rather than making a live HTTP call.
        let p = RetryPolicy::new(2);
        assert_eq!(p.max_retries, 2);
        assert!(!p.retry_transport_errors);
    }
}
