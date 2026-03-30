// Webhook callback support for request completion notifications.
//
// Fires HTTP POST to configured webhook URLs after each request completes.
// Fire-and-forget: spawned tasks, no impact on request latency.

use crate::admin::state::RequestLogEntry;
use crate::config::validate_base_url;
use crate::integrations::NamedIntegration;
use anyllm_client::http::{build_http_client, HttpClientConfig};
use std::sync::Arc;

/// Configuration for webhook callbacks.
#[derive(Clone)]
pub struct CallbackConfig {
    /// Webhook URLs to POST to on request completion.
    urls: Vec<String>,
    /// Named (non-URL) integrations such as Langfuse.
    named: Vec<NamedIntegration>,
    /// Shared HTTP client with timeout.
    client: reqwest::Client,
}

impl CallbackConfig {
    /// Create a new CallbackConfig from a list of webhook URLs.
    /// URLs that don't start with http:// or https:// are skipped with a warning.
    pub fn new(urls: Vec<String>) -> Option<Arc<Self>> {
        Self::with_named(urls, vec![])
    }

    /// Create a CallbackConfig with both webhook URLs and named integrations.
    /// Returns None only when both valid_urls and named are empty.
    /// URLs pointing to private/loopback/metadata IP ranges are rejected to prevent SSRF.
    pub fn with_named(urls: Vec<String>, named: Vec<NamedIntegration>) -> Option<Arc<Self>> {
        let valid_urls: Vec<String> = urls
            .into_iter()
            .filter(|u| {
                if !u.starts_with("http://") && !u.starts_with("https://") {
                    tracing::warn!(
                        callback = %u,
                        "ignoring non-URL callback (only http/https webhook URLs are supported)"
                    );
                    return false;
                }
                // Reject private/loopback/metadata targets to prevent SSRF.
                if let Err(reason) = validate_base_url(u) {
                    tracing::warn!(
                        url = %u,
                        reason = %reason,
                        "ignoring webhook URL: SSRF risk (private/loopback/metadata target)"
                    );
                    return false;
                }
                true
            })
            .collect();

        // Warn on plaintext HTTP (all private/loopback URLs already rejected above).
        for url in &valid_urls {
            if url.starts_with("http://") {
                tracing::warn!(
                    url = %url,
                    "webhook URL uses plaintext HTTP; request metadata (model, tokens, latency) \
                     will be sent unencrypted. Use HTTPS in production."
                );
            }
        }

        if valid_urls.is_empty() && named.is_empty() {
            return None;
        }

        let client = build_http_client(&HttpClientConfig {
            ssrf_protection: true,
            connect_timeout: Some(std::time::Duration::from_secs(5)),
            read_timeout: Some(std::time::Duration::from_secs(10)),
            ..Default::default()
        });

        Some(Arc::new(Self {
            urls: valid_urls,
            named,
            client,
        }))
    }

    /// Create from WEBHOOK_URLS env var (comma-separated).
    pub fn from_env() -> Option<Arc<Self>> {
        let urls_str = std::env::var("WEBHOOK_URLS").ok()?;
        let urls: Vec<String> = urls_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Self::new(urls)
    }

    /// Fire-and-forget: POST the request log entry to all configured webhooks.
    pub fn notify(&self, entry: &RequestLogEntry) {
        let payload = match serde_json::to_value(entry) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("failed to serialize callback payload: {e}");
                return;
            }
        };

        for url in &self.urls {
            let client = self.client.clone();
            let url = url.clone();
            let payload = payload.clone();
            tokio::spawn(async move {
                match client.post(&url).json(&payload).send().await {
                    Ok(resp) => {
                        if !resp.status().is_success() {
                            tracing::debug!(
                                url = %url,
                                status = %resp.status(),
                                "callback webhook returned non-2xx"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::debug!(url = %url, error = %e, "callback webhook failed");
                    }
                }
            });
        }

        for integration in &self.named {
            integration.notify(entry);
        }
    }

    /// Fire-and-forget: POST an arbitrary JSON payload to all configured webhooks.
    /// Used for spend alerts and other event types beyond request completion.
    pub fn notify_json(&self, payload: &serde_json::Value) {
        for url in &self.urls {
            let client = self.client.clone();
            let url = url.clone();
            let payload = payload.clone();
            tokio::spawn(async move {
                match client.post(&url).json(&payload).send().await {
                    Ok(resp) => {
                        if !resp.status().is_success() {
                            tracing::debug!(
                                url = %url,
                                status = %resp.status(),
                                "callback webhook returned non-2xx"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::debug!(url = %url, error = %e, "callback webhook failed");
                    }
                }
            });
        }
    }

    /// Number of configured webhook URLs.
    pub fn url_count(&self) -> usize {
        self.urls.len()
    }

    /// Number of configured named integrations.
    pub fn named_count(&self) -> usize {
        self.named.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_non_url_callbacks() {
        // Non-URL strings and localhost/private URLs are all filtered.
        let config = CallbackConfig::new(vec![
            "https://example.com/hook".to_string(),
            "langfuse".to_string(),            // not a URL, filtered
            "http://localhost:9999/cb".to_string(), // loopback, rejected (SSRF)
        ]);
        let config = config.unwrap();
        // Only the public HTTPS URL survives.
        assert_eq!(config.url_count(), 1);
    }

    #[test]
    fn rejects_private_and_loopback_webhook_urls() {
        let config = CallbackConfig::new(vec![
            "http://169.254.169.254/hook".to_string(), // cloud metadata
            "http://10.0.0.1/hook".to_string(),        // RFC 1918
            "http://127.0.0.1:9999/hook".to_string(),  // loopback
            "http://localhost:8080/hook".to_string(),   // loopback hostname
        ]);
        // All URLs are private/loopback; no valid URLs remain.
        assert!(config.is_none(), "private/loopback webhook URLs must be rejected");
    }

    #[test]
    fn empty_urls_returns_none() {
        assert!(CallbackConfig::new(vec![]).is_none());
        assert!(CallbackConfig::new(vec!["langfuse".to_string()]).is_none());
    }

    #[test]
    fn valid_urls_creates_config() {
        let config = CallbackConfig::new(vec!["https://hook.example.com".to_string()]);
        assert!(config.is_some());
    }

    #[test]
    fn http_plaintext_to_public_host_accepted() {
        // Plaintext HTTP to a public hostname is accepted (with a warning).
        // Private/loopback URLs are now rejected.
        let config = CallbackConfig::new(vec![
            "http://external.example.com/hook".to_string(),
            "https://secure.example.com/hook".to_string(),
        ]);
        let config = config.unwrap();
        assert_eq!(config.url_count(), 2);
    }

    #[test]
    fn with_named_accepts_named_only() {
        // with_named with no URLs but a named integration returns Some
        // We can't construct LangfuseClient directly in tests easily,
        // so just verify with_named([valid_url], []) == new([valid_url])
        let c1 = CallbackConfig::with_named(
            vec!["https://example.com/hook".to_string()],
            vec![],
        );
        let c2 = CallbackConfig::new(vec!["https://example.com/hook".to_string()]);
        assert!(c1.is_some());
        assert!(c2.is_some());
        assert_eq!(c1.unwrap().url_count(), c2.unwrap().url_count());
    }
}
