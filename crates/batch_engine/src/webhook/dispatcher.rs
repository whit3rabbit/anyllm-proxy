// crates/batch_engine/src/webhook/dispatcher.rs
//! Background webhook delivery loop with HMAC signing and retries.

use super::WebhookQueue;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Configuration for the webhook dispatcher.
pub struct WebhookConfig {
    pub poll_interval: Duration,
    pub reclaim_interval: Duration,
    pub max_concurrent: usize,
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1),
            reclaim_interval: Duration::from_secs(30),
            max_concurrent: 8,
        }
    }
}

/// Handle to the running webhook dispatcher.
pub struct WebhookHandle {
    shutdown: CancellationToken,
    join_handle: JoinHandle<()>,
}

impl WebhookHandle {
    pub async fn shutdown(self) {
        self.shutdown.cancel();
        let _ = self.join_handle.await;
    }
}

/// Start the webhook dispatcher background loop.
pub fn start_dispatcher<Q: WebhookQueue>(
    queue: Arc<Q>,
    client: reqwest::Client,
    config: WebhookConfig,
) -> WebhookHandle {
    let shutdown = CancellationToken::new();
    let token = shutdown.clone();

    let join_handle = tokio::spawn(async move {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(config.max_concurrent));
        let mut poll_interval = tokio::time::interval(config.poll_interval);
        let mut reclaim_interval = tokio::time::interval(config.reclaim_interval);

        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                _ = reclaim_interval.tick() => {
                    if let Ok(count) = queue.reclaim_expired_leases().await {
                        if count > 0 {
                            tracing::warn!(count, "reclaimed expired webhook leases");
                        }
                    }
                }
                _ = poll_interval.tick() => {
                    let Ok(permit) = semaphore.clone().try_acquire_owned() else {
                        continue;
                    };

                    match queue.claim_next().await {
                        Ok(Some(leased)) => {
                            let queue = queue.clone();
                            let client = client.clone();
                            tokio::spawn(async move {
                                deliver(queue.as_ref(), &client, &leased.delivery).await;
                                drop(permit);
                            });
                        }
                        Ok(None) => {
                            drop(permit);
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "webhook queue claim error");
                            drop(permit);
                        }
                    }
                }
            }
        }
        tracing::info!("webhook dispatcher shut down");
    });

    WebhookHandle {
        shutdown,
        join_handle,
    }
}

async fn deliver<Q: WebhookQueue>(
    queue: &Q,
    client: &reqwest::Client,
    delivery: &super::WebhookDelivery,
) {
    let mut request = client
        .post(&delivery.url)
        .header("Content-Type", "application/json")
        .header("X-Webhook-Id", &delivery.event_id);

    // HMAC signing.
    if let Some(ref secret) = delivery.signing_secret {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        let payload_bytes = serde_json::to_vec(&delivery.payload).unwrap_or_default();
        let mut mac =
            Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC key length ok");
        mac.update(&payload_bytes);
        let sig = hex::encode(mac.finalize().into_bytes());
        request = request.header("X-Webhook-Signature", format!("sha256={sig}"));
    }

    let response = request.json(&delivery.payload).send().await;

    match response {
        Ok(r) if r.status().is_success() => {
            if let Err(e) = queue.ack(&delivery.delivery_id).await {
                tracing::error!(error = %e, "failed to ack webhook delivery");
            }
        }
        _ => {
            if delivery.attempts < delivery.max_retries {
                let delay = Duration::from_secs(1 << delivery.attempts.min(4));
                if let Err(e) = queue.schedule_retry(&delivery.delivery_id, delay).await {
                    tracing::error!(error = %e, "failed to schedule webhook retry");
                }
            } else {
                tracing::warn!(
                    delivery_id = %delivery.delivery_id,
                    "webhook delivery exhausted retries, moving to dead letter"
                );
                if let Err(e) = queue.dead_letter(&delivery.delivery_id).await {
                    tracing::error!(error = %e, "failed to dead-letter webhook");
                }
            }
        }
    }
}
