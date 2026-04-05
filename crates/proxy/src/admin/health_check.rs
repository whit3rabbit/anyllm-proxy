// Background task: probes each backend every 30 seconds and records results.
// Backend URLs are snapshotted at startup and passed directly; they do not
// change at runtime (base URLs are static config, not admin-mutable).

use crate::admin::db::insert_health_check;
use crate::admin::state::SharedState;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Probe a single backend URL. Returns (is_up, latency_ms).
/// 401 is treated as "up" (server is reachable, just requires auth).
async fn probe_backend(client: &reqwest::Client, base_url: &str) -> (bool, Option<u64>) {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let start = std::time::Instant::now();
    match client
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        Ok(resp) => {
            let latency = start.elapsed().as_millis() as u64;
            let status = resp.status().as_u16();
            let is_up = resp.status().is_success() || matches!(status, 401 | 403 | 404);
            (is_up, Some(latency))
        }
        Err(_) => (false, None),
    }
}

/// Spawns the health-checker loop. Call once at admin server startup.
/// `backend_urls`: snapshot of (backend_name, base_url) pairs from the initial config.
pub fn spawn(shared: SharedState, backend_urls: Vec<(String, String)>) {
    tokio::spawn(async move {
        let client = Arc::new(
            reqwest::Client::builder()
                .timeout(Duration::from_secs(6))
                .build()
                .expect("health check reqwest client"),
        );

        // Track previous up/down state per backend to only broadcast on transitions.
        let mut last_status: HashMap<String, bool> = HashMap::new();

        loop {
            for (name, base_url) in &backend_urls {
                let (is_up, latency_ms) = probe_backend(&client, base_url).await;
                let status_str = if is_up { "up" } else { "down" };

                // Write to DB on the blocking threadpool (rusqlite is sync).
                let name_c = name.clone();
                let latency_c = latency_ms;
                let db = shared.db.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let conn = db.lock().unwrap_or_else(|e| e.into_inner());
                    if let Err(e) = insert_health_check(&conn, &name_c, status_str, latency_c) {
                        tracing::warn!(
                            backend = %name_c,
                            error = %e,
                            "failed to record health check"
                        );
                    }
                })
                .await;

                // Broadcast only on state transitions (up->down or down->up).
                let prev = last_status.insert(name.clone(), is_up);
                if prev != Some(is_up) {
                    let _ =
                        shared
                            .events_tx
                            .send(crate::admin::state::AdminEvent::BackendHealthChanged {
                                backend: name.clone(),
                                status: status_str.to_string(),
                                latency_ms,
                            });
                }
            }

            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}
