use anyllm_proxy::admin;
use std::sync::Arc;

pub(crate) fn spawn_background_pruner(
    virtual_keys: Arc<dashmap::DashMap<[u8; 32], admin::keys::VirtualKeyMeta>>,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            interval.tick().await;
            let now = anyllm_proxy::admin::keys::now_ms();
            let now_secs = (now / 1000) as i64;
            virtual_keys.retain(|_, v| {
                let _ = v.rate_state.check_rpm(0, now);
                let _ = v.rate_state.check_tpm(0, now);
                v.expires_at.is_none_or(|exp| now_secs < exp)
            });
        }
    });
}

pub(crate) fn spawn_auto_refresh_task(
    shared: admin::state::SharedState,
    refresh_interval_hours: u64,
) {
    let client = crate::main_helpers::providers_cmd::PROVIDER_REFRESH_CLIENT.clone();
    tokio::spawn(async move {
        let interval = std::time::Duration::from_secs(refresh_interval_hours * 3600);
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        loop {
            let providers: Vec<_> = shared.provider_catalog.all_providers().cloned().collect();
            for provider in providers {
                if !provider.capabilities.chat_completions {
                    continue;
                }
                if provider.default_base_url.is_empty() {
                    continue;
                }
                let api_key = provider
                    .env_vars
                    .iter()
                    .find_map(|v| std::env::var(v.as_str()).ok());
                if api_key.is_none() {
                    continue;
                }

                let url = format!(
                    "{}/v1/models",
                    provider.default_base_url.trim_end_matches('/')
                );
                let provider_id = provider.id.clone();
                let mut req = client.get(&url);
                if let Some(ref key) = api_key {
                    req = req.header("Authorization", format!("Bearer {key}"));
                }
                match req.send().await {
                    Err(e) => tracing::warn!(
                        provider = %provider_id,
                        error = %e,
                        "provider auto-refresh failed"
                    ),
                    Ok(resp) if !resp.status().is_success() => tracing::warn!(
                        provider = %provider_id,
                        status = %resp.status(),
                        "provider auto-refresh upstream error"
                    ),
                    Ok(resp) => match resp.json::<serde_json::Value>().await {
                        Err(e) => tracing::warn!(
                            provider = %provider_id,
                            error = %e,
                            "provider auto-refresh: invalid JSON response"
                        ),
                        Ok(json) => {
                            let model_ids: Vec<String> = json
                                .get("data")
                                .and_then(|d| d.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|m| m.get("id")?.as_str().map(String::from))
                                        .collect()
                                })
                                .unwrap_or_default();
                            let count = model_ids.len();
                            let db_ref = shared.db.clone();
                            let pid = provider_id.clone();
                            let _ = tokio::task::spawn_blocking(move || {
                                let mut conn_guard =
                                    db_ref.lock().unwrap_or_else(|e| e.into_inner());
                                if let Err(e) = admin::db::upsert_provider_models_cache(
                                    &mut conn_guard,
                                    &pid,
                                    &model_ids,
                                ) {
                                    tracing::warn!(
                                        provider = %pid,
                                        error = %e,
                                        "failed to save auto-refresh results"
                                    );
                                }
                            })
                            .await;
                            tracing::info!(
                                provider = %provider_id,
                                count = count,
                                "auto-refreshed provider model cache"
                            );
                        }
                    },
                }
            }
            tokio::time::sleep(interval).await;
        }
    });
}

pub(crate) fn spawn_periodic_tasks(shared: admin::state::SharedState, retention_days: u32) {
    // 1. Log retention task
    let retention_db = shared.db.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            interval.tick().await;
            admin::state::with_db(
                &retention_db,
                move |conn_ref| match admin::db::purge_old_logs(conn_ref, retention_days) {
                    Ok(n) if n > 0 => {
                        tracing::info!(purged = n, "purged old request log entries")
                    }
                    Err(e) => tracing::error!(error = %e, "failed to purge old logs"),
                    _ => {}
                },
            )
            .await;
        }
    });

    // 2. Periodic metrics snapshot broadcast task
    let snapshot_shared = shared.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            if snapshot_shared.events_tx.receiver_count() == 0 {
                continue;
            }
            let mut aggregate = anyllm_proxy::metrics::MetricsSnapshot::default();
            for m in snapshot_shared.backend_metrics.values() {
                let snap = m.snapshot();
                aggregate.requests_total += snap.requests_total;
                aggregate.requests_error += snap.requests_error;
                aggregate.requests_success += snap.requests_success;
                aggregate.streams_started += snap.streams_started;
                aggregate.streams_completed += snap.streams_completed;
                aggregate.streams_failed += snap.streams_failed;
                aggregate.streams_client_disconnected += snap.streams_client_disconnected;
            }
            let error_rate = aggregate.error_rate();
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let since = now_secs.saturating_sub(60);
            let rpm = admin::state::with_db(&snapshot_shared.db, move |conn_ref| {
                admin::db::count_requests_since(conn_ref, since).unwrap_or(0)
            })
            .await
            .unwrap_or(0) as f64;

            let snapshot = admin::state::MetricsSnapshotData {
                total_requests: aggregate.requests_total,
                successful_requests: aggregate.requests_success,
                failed_requests: aggregate.requests_error,
                requests_per_minute: rpm,
                p50_latency_ms: None,
                p95_latency_ms: None,
                error_rate,
                streams_started: aggregate.streams_started,
                streams_completed: aggregate.streams_completed,
                streams_failed: aggregate.streams_failed,
                streams_client_disconnected: aggregate.streams_client_disconnected,
            };
            let _ = snapshot_shared
                .events_tx
                .send(admin::state::AdminEvent::MetricsSnapshot(snapshot));
        }
    });
}
