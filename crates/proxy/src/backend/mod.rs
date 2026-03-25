pub mod anthropic_client;
pub mod openai_client;

use crate::config::{BackendAuth, BackendConfig, BackendKind, Config, OpenAIApiFormat, TlsConfig};
use anthropic_client::{AnthropicClient, AnthropicClientError};
use axum::http::{HeaderMap, HeaderName, HeaderValue};
use openai_client::{OpenAIClient, OpenAIClientError};
use reqwest::Client;
use serde::Serialize;
use std::time::Duration;
use tokio::time::sleep;

/// DNS resolver that rejects private/loopback IPs at connection time,
/// preventing DNS rebinding attacks where a domain resolves to a public IP
/// at startup validation but later resolves to a private/metadata IP.
struct SsrfSafeDnsResolver;

impl reqwest::dns::Resolve for SsrfSafeDnsResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            let name_str = name.as_str().to_string();
            // DNS resolution (ToSocketAddrs) blocks the calling thread.
            // Must run on the blocking threadpool to avoid stalling the
            // async runtime and all other in-flight requests.
            let addrs: Vec<std::net::SocketAddr> =
                tokio::task::spawn_blocking(move || -> Result<Vec<std::net::SocketAddr>, _> {
                    use std::net::ToSocketAddrs;
                    // Port 0 is a placeholder; reqwest replaces it with the actual port.
                    let lookup = format!("{name_str}:0");
                    Ok(lookup.to_socket_addrs()?.collect())
                })
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?
                .map_err(
                    |e: std::io::Error| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) },
                )?;

            // Filter out private/loopback IPs to prevent SSRF attacks where
            // an attacker-controlled DNS record resolves to internal endpoints
            // (e.g., cloud metadata at 169.254.169.254).
            let safe: Vec<std::net::SocketAddr> = addrs
                .into_iter()
                .filter(|addr| !crate::config::is_private_ip(addr.ip()))
                .collect();

            if safe.is_empty() {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "DNS resolved only to private/loopback IPs (SSRF blocked)".to_string(),
                ))
                    as Box<dyn std::error::Error + Send + Sync>);
            }

            Ok(Box::new(safe.into_iter()) as Box<dyn Iterator<Item = std::net::SocketAddr> + Send>)
        })
    }
}

pub(crate) const MAX_RETRIES: u32 = 3;
pub(crate) const BASE_DELAY_MS: u64 = 500;

/// Backend error types implement this to enable the generic `send_with_retry`.
pub(crate) trait RetryableError: Sized {
    fn from_request(e: reqwest::Error) -> Self;
    fn from_api_response(status: u16, body: &str) -> Self;
}

fn apply_auth(rb: reqwest::RequestBuilder, auth: &BackendAuth) -> reqwest::RequestBuilder {
    match auth {
        BackendAuth::BearerToken(token) => rb.bearer_auth(token),
        BackendAuth::GoogleApiKey(key) => rb.header("x-goog-api-key", key),
    }
}

/// Send a POST request with retry on 429/5xx. Returns the raw successful response.
pub(crate) async fn send_with_retry<E: RetryableError>(
    client: &Client,
    url: &str,
    auth: &BackendAuth,
    body: &impl Serialize,
    label: &str,
) -> Result<reqwest::Response, E> {
    for attempt in 0..=MAX_RETRIES {
        let rb = apply_auth(client.post(url).json(body), auth);
        let response = rb.send().await.map_err(E::from_request)?;
        let status = response.status().as_u16();

        if (200..300).contains(&status) {
            return Ok(response);
        }

        if attempt < MAX_RETRIES && is_retryable(status) {
            let retry_after = parse_retry_after(response.headers());
            let delay = backoff_delay(attempt, retry_after);
            tracing::warn!(
                status,
                attempt = attempt + 1,
                max_retries = MAX_RETRIES,
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

    unreachable!("loop runs MAX_RETRIES+1 times and always returns")
}

/// Build a reqwest HTTP client with optional mTLS identity and custom CA cert.
pub(crate) fn build_http_client(tls: &TlsConfig) -> Client {
    let mut builder = Client::builder();

    if let Some((ref p12_bytes, ref password)) = tls.p12_identity {
        let identity = reqwest::Identity::from_pkcs12_der(p12_bytes, password)
            .expect("P12 identity was validated at startup");
        builder = builder.identity(identity);
    }

    if let Some(ref ca_pem) = tls.ca_cert_pem {
        let cert =
            reqwest::Certificate::from_pem(ca_pem).expect("CA cert was validated at startup");
        builder = builder.add_root_certificate(cert);
    }

    builder
        .connect_timeout(Duration::from_secs(10))
        // 15 min read timeout: generous for slow-starting reasoning models
        // (o1/o3 can think >5 min before the first chunk) while still
        // bounding hung connections that would otherwise pin resources.
        .read_timeout(Duration::from_secs(900))
        // Detect dead TCP connections (peer crash, network drop).
        .tcp_keepalive(Duration::from_secs(60))
        // Validate resolved IPs at connection time to prevent DNS rebinding SSRF.
        .dns_resolver(std::sync::Arc::new(SsrfSafeDnsResolver))
        .build()
        .expect("failed to build HTTP client")
}

/// Check if a status code is retryable (408, 429, or 5xx).
pub(crate) fn is_retryable(status: u16) -> bool {
    status == 408 || status == 429 || (500..=599).contains(&status)
}

/// Parse retry-after header as integer seconds or HTTP date (RFC 7231).
pub(crate) fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let value = header_str(headers, "retry-after")?;
    // Fast path: integer seconds
    if let Ok(secs) = value.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    // HTTP date (RFC 7231). Past dates return None (no wait needed).
    let date = httpdate::parse_http_date(&value).ok()?;
    date.duration_since(std::time::SystemTime::now()).ok()
}

/// Compute backoff delay with jitter.
pub(crate) fn backoff_delay(attempt: u32, retry_after: Option<Duration>) -> Duration {
    if let Some(ra) = retry_after {
        return ra;
    }
    let base = Duration::from_millis(BASE_DELAY_MS * 2u64.pow(attempt));
    // Deterministic 25% jitter (upper bound, not random) to keep tests
    // predictable while still spreading retry storms across backends.
    let jitter_ms = (base.as_millis() as u64) / 4;
    base + Duration::from_millis(jitter_ms)
}

/// Backend-agnostic client for dispatching requests to OpenAI, Vertex, Gemini, or Anthropic.
/// Callers pattern-match on the enum variants to access the typed inner clients directly.
#[derive(Clone)]
pub enum BackendClient {
    OpenAI(OpenAIClient),
    /// Same HTTP client as OpenAI, but targets the Responses API endpoint
    /// with a different request/response shape. Separate variant so callers
    /// can pattern-match on the API format.
    OpenAIResponses(OpenAIClient),
    Vertex(OpenAIClient),
    /// Gemini via OpenAI-compatible endpoint (reuses OpenAI translation path).
    GeminiOpenAI(OpenAIClient),
    /// Passthrough to real Anthropic API (no translation).
    Anthropic(AnthropicClient),
}

/// Unified error type for all backend clients.
#[derive(Debug)]
pub enum BackendError {
    OpenAI(OpenAIClientError),
    Anthropic(AnthropicClientError),
}

impl BackendError {
    /// HTTP status code for API errors, None for transport/deserialization errors.
    pub fn api_error_status(&self) -> Option<u16> {
        match self {
            Self::OpenAI(OpenAIClientError::ApiError { status, .. }) => Some(*status),
            _ => None,
        }
    }

    /// HTTP status code from an API error, or 500 for transport/deserialization errors.
    pub fn status_code(&self) -> u16 {
        self.api_error_status().unwrap_or(500)
    }

    /// Human-readable error message.
    pub fn api_error_message(&self) -> String {
        match self {
            Self::OpenAI(e) => e.to_string(),
            Self::Anthropic(e) => e.to_string(),
        }
    }

    /// Extract the upstream error message and HTTP status for API errors.
    /// Returns None for transport/deserialization errors.
    pub fn api_error_details(&self) -> Option<(&str, u16)> {
        match self {
            Self::OpenAI(OpenAIClientError::ApiError { status, error }) => {
                Some((&error.error.message, *status))
            }
            _ => None,
        }
    }
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpenAI(e) => write!(f, "{e}"),
            Self::Anthropic(e) => write!(f, "{e}"),
        }
    }
}

impl From<OpenAIClientError> for BackendError {
    fn from(e: OpenAIClientError) -> Self {
        Self::OpenAI(e)
    }
}

impl From<AnthropicClientError> for BackendError {
    fn from(e: AnthropicClientError) -> Self {
        Self::Anthropic(e)
    }
}

impl BackendClient {
    pub fn new(config: &Config) -> Self {
        match config.backend {
            BackendKind::OpenAI => match config.openai_api_format {
                OpenAIApiFormat::Chat => Self::OpenAI(OpenAIClient::new(config)),
                OpenAIApiFormat::Responses => Self::OpenAIResponses(OpenAIClient::new(config)),
            },
            BackendKind::Vertex => Self::Vertex(OpenAIClient::new(config)),
            BackendKind::Gemini => Self::GeminiOpenAI(OpenAIClient::new(config)),
            BackendKind::Anthropic => Self::Anthropic(AnthropicClient::new(
                &config.openai_base_url,
                &config.openai_api_key,
                &config.tls,
            )),
        }
    }

    /// Construct from a per-backend config (multi-backend mode).
    pub fn from_backend_config(bc: &BackendConfig) -> Self {
        // Build a legacy Config to reuse existing OpenAI constructors.
        // This avoids duplicating URL construction logic.
        let legacy = Config {
            backend: bc.kind.clone(),
            openai_api_key: bc.api_key.clone(),
            openai_base_url: bc.base_url.clone(),
            listen_port: 0, // unused by client constructors
            model_mapping: bc.model_mapping.clone(),
            tls: bc.tls.clone(),
            backend_auth: bc.backend_auth.clone(),
            log_bodies: bc.log_bodies,
            openai_api_format: bc.api_format.clone(),
        };

        match bc.kind {
            BackendKind::OpenAI => match bc.api_format {
                OpenAIApiFormat::Chat => Self::OpenAI(OpenAIClient::new(&legacy)),
                OpenAIApiFormat::Responses => Self::OpenAIResponses(OpenAIClient::new(&legacy)),
            },
            BackendKind::Vertex => Self::Vertex(OpenAIClient::new(&legacy)),
            BackendKind::Gemini => Self::GeminiOpenAI(OpenAIClient::new(&legacy)),
            BackendKind::Anthropic => Self::Anthropic(AnthropicClient::from_backend_config(bc)),
        }
    }
}

/// Parse OpenAI's duration format (e.g., "6ms", "1s", "1m30s", "2m") into a
/// [`Duration`]. Returns `None` for unrecognized formats.
fn parse_openai_duration(s: &str) -> Option<Duration> {
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

/// Convert an OpenAI relative duration string to an ISO 8601 UTC timestamp
/// by adding it to the current time. Returns `None` if parsing fails.
///
/// Accepts an `anchor` time so callers can pin the base for testability.
fn openai_duration_to_iso8601_at(s: &str, anchor: std::time::SystemTime) -> Option<String> {
    let dur = parse_openai_duration(s)?;
    let reset_time = anchor + dur;
    let secs = reset_time
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(crate::admin::db::epoch_to_iso8601(secs))
}

/// Convenience wrapper using the current time as anchor.
fn openai_duration_to_iso8601(s: &str) -> Option<String> {
    openai_duration_to_iso8601_at(s, std::time::SystemTime::now())
}

/// Convert an OpenAI relative duration to ISO 8601, falling back to the raw
/// value with a warning if parsing fails.
fn convert_reset_duration(raw: &Option<String>, field: &str) -> Option<String> {
    raw.as_deref().map(|v| {
        openai_duration_to_iso8601(v).unwrap_or_else(|| {
            tracing::warn!(value = v, field, "failed to parse reset duration, forwarding raw");
            v.to_string()
        })
    })
}

/// Rate limit headers extracted from backend responses.
#[derive(Debug, Default, Clone)]
pub struct RateLimitHeaders {
    pub requests_limit: Option<String>,
    pub requests_remaining: Option<String>,
    pub requests_reset: Option<String>,
    pub tokens_limit: Option<String>,
    pub tokens_remaining: Option<String>,
    pub tokens_reset: Option<String>,
    pub retry_after: Option<String>,
}

/// Extract a header value as a trimmed string.
fn header_str(headers: &reqwest::header::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
}

/// Set a header on an axum HeaderMap if the value is Some.
fn set_if_some(map: &mut HeaderMap, name: &str, value: &Option<String>) {
    if let Some(v) = value {
        if let (Ok(header_name), Ok(header_value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            axum::http::HeaderValue::from_str(v),
        ) {
            map.insert(header_name, header_value);
        }
    }
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
        }
    }

    /// Inject Anthropic-format response headers (rate limits + version) into a HeaderMap.
    ///
    /// The `*_reset` fields are converted from OpenAI's relative duration
    /// format (e.g., "1s") to Anthropic's ISO 8601 UTC timestamp format.
    /// Falls back to the raw value with a warning if parsing fails.
    pub fn inject_anthropic_response_headers(&self, map: &mut HeaderMap) {
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
        map.insert(
            HeaderName::from_static("anthropic-version"),
            HeaderValue::from_static("2023-06-01"),
        );
    }
}

#[cfg(test)]
mod rate_limit_tests {
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
        let mut map = HeaderMap::new();
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
        // Fields that were None should not be present
        assert!(map.get("anthropic-ratelimit-requests-remaining").is_none());
        assert!(map.get("anthropic-ratelimit-tokens-limit").is_none());
    }

    #[test]
    fn inject_anthropic_response_headers_default_sets_version_only() {
        let rl = RateLimitHeaders::default();
        let mut map = HeaderMap::new();
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
        let mut map = HeaderMap::new();
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
        // Should be ISO 8601 format, not the raw OpenAI duration.
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
        assert_eq!(
            parse_openai_duration("6ms"),
            Some(Duration::from_millis(6))
        );
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
        assert_eq!(parse_openai_duration("123"), None); // no unit
        assert_eq!(parse_openai_duration("1x"), None); // unknown unit
    }

    #[test]
    fn openai_duration_to_iso8601_at_pinned_time() {
        // 2025-06-16T12:00:00Z = 1750075200 epoch seconds
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
            "2025-06-16T12:00:00Z" // 500ms rounds down to same second
        );
    }

    #[test]
    fn openai_duration_to_iso8601_invalid_returns_none() {
        assert!(openai_duration_to_iso8601("garbage").is_none());
        assert!(openai_duration_to_iso8601("").is_none());
    }
}
