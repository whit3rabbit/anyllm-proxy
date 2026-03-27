// Webhook callback support for request completion notifications.
//
// Fires HTTP POST to configured webhook URLs after each request completes.
// Fire-and-forget: spawned tasks, no impact on request latency.

use crate::admin::state::RequestLogEntry;
use std::sync::Arc;

/// Configuration for webhook callbacks.
#[derive(Clone)]
pub struct CallbackConfig {
    /// Webhook URLs to POST to on request completion.
    urls: Vec<String>,
    /// Shared HTTP client with timeout.
    client: reqwest::Client,
}

impl CallbackConfig {
    /// Create a new CallbackConfig from a list of webhook URLs.
    /// URLs that don't start with http:// or https:// are skipped with a warning.
    pub fn new(urls: Vec<String>) -> Option<Arc<Self>> {
        let valid_urls: Vec<String> = urls
            .into_iter()
            .filter(|u| {
                if u.starts_with("http://") || u.starts_with("https://") {
                    true
                } else {
                    tracing::warn!(
                        callback = %u,
                        "ignoring non-URL callback (only http/https webhook URLs are supported)"
                    );
                    false
                }
            })
            .collect();

        if valid_urls.is_empty() {
            return None;
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("callback http client");

        Some(Arc::new(Self {
            urls: valid_urls,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_non_url_callbacks() {
        let config = CallbackConfig::new(vec![
            "https://example.com/hook".to_string(),
            "langfuse".to_string(), // not a URL, should be filtered
            "http://localhost:9999/cb".to_string(),
        ]);
        let config = config.unwrap();
        assert_eq!(config.url_count(), 2);
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
}
