// Background task: probes each backend every 30 seconds and records results.
// Backend URLs are snapshotted at startup and passed directly; they do not
// change at runtime (base URLs are static config, not admin-mutable).

use crate::admin::db::insert_health_check;
use crate::admin::state::SharedState;
use std::collections::HashMap;
use std::time::Duration;
use tokio::task::JoinSet;

/// Probe a single backend URL. Returns (is_up, latency_ms).
/// 401/403/404 are treated as "up" (server is reachable, just requires auth).
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
        // reqwest::Client is Arc-based internally; clone() shares the connection pool.
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(6))
            .build()
            .expect("health check reqwest client");

        // Track previous up/down state per backend to only broadcast on transitions.
        let mut last_status: HashMap<String, bool> = HashMap::new();

        loop {
            // Probe all backends concurrently so a slow/down backend doesn't delay others.
            let mut set: JoinSet<(String, bool, Option<u64>)> = JoinSet::new();
            for (name, base_url) in backend_urls.iter().cloned() {
                let c = client.clone();
                set.spawn(async move {
                    let (is_up, latency_ms) = probe_backend(&c, &base_url).await;
                    (name, is_up, latency_ms)
                });
            }

            while let Some(Ok((name, is_up, latency_ms))) = set.join_next().await {
                let status_str = if is_up { "up" } else { "down" };

                // Write to DB on the blocking threadpool (rusqlite is sync).
                let db = shared.db.clone();
                let _ = tokio::task::spawn_blocking({
                    let name = name.clone();
                    move || {
                        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
                        if let Err(e) = insert_health_check(&conn, &name, status_str, latency_ms) {
                            tracing::warn!(backend = %name, error = %e, "failed to record health check");
                        }
                    }
                })
                .await;

                // Broadcast only on state transitions (up->down or down->up).
                let prev = last_status.insert(name.clone(), is_up);
                if prev != Some(is_up) {
                    let _ = shared.events_tx.send(
                        crate::admin::state::AdminEvent::BackendHealthChanged {
                            backend: name,
                            status: status_str.to_string(),
                            latency_ms,
                        },
                    );
                }
            }

            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}
