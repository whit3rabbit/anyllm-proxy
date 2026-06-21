use anyllm_proxy::admin;
use anyllm_proxy::config;
use anyllm_proxy::server::state as server_state;
use std::path::Path;
use std::sync::Arc;

#[allow(clippy::too_many_lines)]
pub(crate) async fn init_admin(
    args: &[String],
    data_dir: &Path,
    multi_config: &config::MultiConfig,
    model_router: Option<Arc<std::sync::RwLock<config::model_router::ModelRouter>>>,
    tool_engine_state: &Option<Arc<server_state::ToolEngineState>>,
    reload_handle: tracing_subscriber::reload::Handle<
        tracing_subscriber::EnvFilter,
        tracing_subscriber::Registry,
    >,
) -> Option<(
    admin::state::SharedState,
    axum::Router,
    tokio::net::TcpListener,
    u16,
)> {
    let flag_set = args.iter().any(|a| a == "--webui" || a == "--admin");
    let force_disabled = matches!(
        std::env::var("DISABLE_ADMIN").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    );
    let enable_admin = flag_set && !force_disabled;

    if !enable_admin {
        return None;
    }

    let provider_catalog = Arc::new(anyllm_providers::ProviderCatalog::bundled());

    let admin_port: u16 = match std::env::var("ADMIN_PORT") {
        Ok(val) => val
            .parse::<u16>()
            .unwrap_or_else(|_| panic!("ADMIN_PORT must be a number in 1-65535, got '{val}'")),
        Err(_) => 3001,
    };
    if admin_port == 0 {
        panic!("ADMIN_PORT cannot be 0");
    }
    if admin_port < 1024 {
        tracing::warn!(
            port = admin_port,
            "ADMIN_PORT is in the privileged range (< 1024); binding may fail without elevated privileges"
        );
    }

    let admin_bind = {
        let raw = std::env::var("ADMIN_BIND").unwrap_or_else(|_| "127.0.0.1".into());
        // Translate well-known loopback aliases to explicit IPs before validation.
        match raw.to_ascii_lowercase().as_str() {
            "localhost" => "127.0.0.1".to_string(),
            "localhost6" | "ip6-localhost" | "ip6-loopback" => "::1".to_string(),
            _ => raw,
        }
    };
    // Bind addresses must be explicit IPs. Hostnames other than the loopback
    // aliases above are rejected: a bind address maps to a specific local
    // interface and must be unambiguous.
    if admin_bind.parse::<std::net::IpAddr>().is_err() {
        panic!(
            "ADMIN_BIND must be an IP address (e.g. 127.0.0.1 or 0.0.0.0), not a hostname — got '{}'",
            std::env::var("ADMIN_BIND").unwrap_or_default()
        );
    }

    if admin_port == multi_config.listen_port {
        panic!(
            "ADMIN_PORT ({admin_port}) must differ from LISTEN_PORT ({})",
            multi_config.listen_port
        );
    }

    let db_path = crate::main_helpers::bootstrap::resolve_db_path(data_dir);
    let conn =
        rusqlite::Connection::open(&db_path).expect("failed to open SQLite database for admin");
    admin::db::init_db(&conn).expect("failed to initialize admin database schema");
    let hmac_secret = Arc::new(admin::db::ensure_hmac_secret(&conn));

    // Build initial RuntimeConfig from the loaded multi_config.
    let mut model_mappings = indexmap::IndexMap::new();
    for (name, bc) in &multi_config.backends {
        model_mappings.insert(name.clone(), bc.model_mapping.clone());
    }
    let log_level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into());
    let mut runtime_config = admin::state::RuntimeConfig {
        model_mappings,
        log_level,
        log_bodies: multi_config.log_bodies,
        redact_secrets: multi_config.redact_secrets,
    };
    let runtime_defaults = admin::state::RuntimeConfigDefaults {
        log_bodies: multi_config.log_bodies,
        redact_secrets: multi_config.redact_secrets,
    };
    let mut log_bodies_enabled_by_override = false;
    let mut redact_secrets_enabled_by_override = false;

    // Apply config overrides from SQLite (survive restarts).
    if let Ok(overrides) = admin::db::get_config_overrides(&conn) {
        for (key, value, _) in &overrides {
            match key.as_str() {
                "log_level" => {
                    // Apply the same allowlist enforced by the admin API to
                    // prevent a tampered SQLite database from enabling trace-level
                    // logging, which would expose API keys in HTTP headers.
                    const ALLOWED_LOG_LEVELS: &[&str] = &["error", "warn", "info", "debug"];
                    let normalized = value.trim().to_lowercase();
                    if ALLOWED_LOG_LEVELS.contains(&normalized.as_str()) {
                        runtime_config.log_level = normalized;
                    } else {
                        tracing::warn!(
                            value = %value,
                            "ignoring invalid log_level override from database"
                        );
                    }
                }
                "log_bodies" => {
                    runtime_config.log_bodies = value == "true";
                    log_bodies_enabled_by_override =
                        runtime_config.log_bodies && !multi_config.log_bodies;
                }
                "redact_secrets" => {
                    runtime_config.redact_secrets = value == "true";
                    redact_secrets_enabled_by_override =
                        runtime_config.redact_secrets && !multi_config.redact_secrets;
                }
                k if k.ends_with(".big_model") => {
                    let backend = k.strip_suffix(".big_model").unwrap();
                    if let Some(m) = runtime_config.model_mappings.get_mut(backend) {
                        m.big_model = value.clone();
                    }
                }
                k if k.ends_with(".small_model") => {
                    let backend = k.strip_suffix(".small_model").unwrap();
                    if let Some(m) = runtime_config.model_mappings.get_mut(backend) {
                        m.small_model = value.clone();
                    }
                }
                _ => {
                    tracing::debug!(key = %key, "unknown config override, skipping");
                }
            }
        }
        if !overrides.is_empty() {
            tracing::info!(
                count = overrides.len(),
                "applied config overrides from database"
            );
        }
    }

    if log_bodies_enabled_by_override {
        tracing::warn!(
            "persisted admin override enabled LOG_BODIES: request and response bodies will be \
             logged at debug level. This may expose sensitive data (prompts, API keys, PII)."
        );
    }
    if redact_secrets_enabled_by_override {
        tracing::warn!(
            "persisted admin override enabled REDACT_SECRETS: upstream JSON/text request payloads \
             will be scanned and detected secrets will be replaced before forwarding."
        );
    }
    if let Err(message) =
        anyllm_proxy::server::ensure_secret_redaction_available(runtime_config.redact_secrets)
    {
        panic!("{}", message);
    }

    let runtime_config = Arc::new(std::sync::RwLock::new(runtime_config));

    // Build the log_reload closure that captures the reload handle.
    // We get this reload handle from caller or trace initialization.
    // Note: reload_handle is captured dynamically during async_main setup,
    // so we can resolve this by passing reload_handle, or keeping the logic
    // in main mod.rs. Let's pass the log_reload closure or reload_handle in.
    // Actually, let's define reload_handle as a parameter to init_admin, or pass
    // log_reload in.
    // Let's pass:
    // log_reload: Arc<dyn Fn(&str) -> bool + Send + Sync>

    // Load active virtual keys from SQLite into in-memory DashMap.
    let virtual_keys = Arc::new(dashmap::DashMap::new());
    {
        if let Ok(active_keys) = admin::db::load_active_virtual_keys(&conn) {
            for key_row in &active_keys {
                if let Some(hash_bytes) = admin::keys::hash_from_hex(&key_row.key_hash) {
                    virtual_keys.insert(
                        hash_bytes,
                        admin::keys::VirtualKeyMeta {
                            id: key_row.id,
                            description: key_row.description.clone(),
                            expires_at: key_row.expires_at.as_deref().and_then(|s| {
                                anyllm_proxy::integrations::langfuse::iso8601_to_epoch(s)
                                    .and_then(|e| i64::try_from(e).ok())
                            }),
                            rpm_limit: key_row.rpm_limit,
                            tpm_limit: key_row.tpm_limit,
                            rate_state: Arc::new(admin::keys::RateLimitState::new()),
                            role: admin::keys::KeyRole::from_str_or_default(&key_row.role),
                            max_budget_usd: key_row.max_budget_usd,
                            budget_duration: key_row
                                .budget_duration
                                .as_deref()
                                .and_then(admin::keys::BudgetDuration::parse),
                            period_start: key_row.period_start.clone(),
                            period_spend_usd: key_row.period_spend_usd,
                            allowed_models: key_row.allowed_models.clone(),
                            allowed_routes: key_row.allowed_routes.clone(),
                        },
                    );
                }
            }
            tracing::info!(
                count = active_keys.len(),
                "loaded virtual API keys from database"
            );
        }
    }

    // Make virtual keys and HMAC secret available to the auth middleware.
    anyllm_proxy::server::middleware::set_virtual_keys(virtual_keys.clone());
    anyllm_proxy::server::middleware::set_hmac_secret(hmac_secret.clone());

    let virtual_keys_pruner = virtual_keys.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            interval.tick().await;
            let now = anyllm_proxy::admin::keys::now_ms();
            // Single pass: slide rate-limit windows forward (frees old
            // buckets) and drop expired keys. Without active eviction,
            // expired keys accumulate in the DashMap until next auth use.
            let now_secs = (now / 1000) as i64;
            virtual_keys_pruner.retain(|_, v| {
                let _ = v.rate_state.check_rpm(0, now);
                let _ = v.rate_state.check_tpm(0, now);
                v.expires_at.is_none_or(|exp| now_secs < exp)
            });
        }
    });

    // Load managed backends from SQLite into in-memory HashMap.
    let managed_backends = {
        let mut map = std::collections::HashMap::new();
        if let Ok(rows) = admin::db::list_managed_backends(&conn) {
            for row in rows {
                match provider_catalog.get_provider(&row.provider_id) {
                    None => {
                        tracing::warn!(
                            provider_id = %row.provider_id,
                            backend_id = %row.id,
                            "managed backend references unknown provider; skipping"
                        );
                    }
                    Some(provider) => {
                        match anyllm_proxy::admin::routes::managed_backends::row_to_backend_config(
                            &row, provider,
                        ) {
                            Err(e) => {
                                tracing::warn!(
                                    provider_id = %row.provider_id,
                                    backend_id = %row.id,
                                    error = %e.message(),
                                    "managed backend configuration is invalid; skipping"
                                );
                            }
                            Ok(bc) => {
                                let client =
                                    anyllm_proxy::backend::BackendClient::from_backend_config(&bc);
                                map.insert(row.name.clone(), (row, client));
                            }
                        }
                    }
                }
            }
        }
        tracing::info!(count = map.len(), "loaded managed backends from SQLite");
        Arc::new(std::sync::RwLock::new(map))
    };

    let db = Arc::new(std::sync::Mutex::new(conn));
    let (events_tx, _) = tokio::sync::broadcast::channel(1024);
    let log_tx = admin::db::spawn_write_buffer(db.clone());

    let backend_metrics: std::collections::HashMap<String, anyllm_proxy::metrics::Metrics> =
        std::collections::HashMap::new();

    let log_reload: Arc<dyn Fn(&str) -> bool + Send + Sync> = {
        let handle = reload_handle;
        Arc::new(
            move |new_filter: &str| match tracing_subscriber::EnvFilter::try_new(new_filter) {
                Ok(f) => handle.reload(f).is_ok(),
                Err(e) => {
                    tracing::error!(filter = new_filter, error = %e, "invalid log filter string");
                    false
                }
            },
        )
    };

    // Admin token: use env var or generate 256-bit random hex written to a file.
    let admin_token = match std::env::var("ADMIN_TOKEN") {
        Ok(t) => {
            if t.len() < 32 {
                tracing::warn!(
                    len = t.len(),
                    "ADMIN_TOKEN is shorter than 32 characters; use a longer random value to reduce brute-force risk (generate one with: openssl rand -hex 32)"
                );
            }
            t
        }
        Err(_) => {
            let mut buf = [0u8; 32];
            getrandom::fill(&mut buf).expect("getrandom failed");
            let token = hex::encode(buf);
            let token_path = crate::main_helpers::bootstrap::resolve_admin_token_path(data_dir);
            let token_path_str = token_path.to_string_lossy().to_string();
            // Write token to file with restrictive permissions instead of stderr
            if let Err(e) =
                crate::main_helpers::bootstrap::write_token_file(&token_path_str, &token)
            {
                panic!(
                    "Cannot write admin token to {token_path_str}: {e}. Set ADMIN_TOKEN env var explicitly or ensure the path is writable."
                );
            } else {
                tracing::info!(
                    path = %token_path_str,
                    "admin token written to file — retrieve with: cat {token_path_str} | set ADMIN_TOKEN env var to use a fixed token across restarts"
                );
            }
            token
        }
    };
    let admin_token = Arc::new(zeroize::Zeroizing::new(admin_token));

    let shared = admin::state::SharedState {
        db: db.clone(),
        events_tx: events_tx.clone(),
        runtime_config: runtime_config.clone(),
        runtime_defaults,
        backend_metrics: Arc::new(backend_metrics),
        log_tx,
        log_reload: Some(log_reload),
        config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
        virtual_keys,
        hmac_secret,
        model_router: model_router.clone(),
        provider_catalog: provider_catalog.clone(),
        mcp_manager: tool_engine_state
            .as_ref()
            .and_then(|s| s.mcp_manager.clone()),
        issued_csrf_tokens: Arc::new(
            moka::sync::Cache::builder()
                .max_capacity(1_000)
                .time_to_live(std::time::Duration::from_secs(86400))
                .build(),
        ),
        started_at: std::time::SystemTime::now(),
        managed_backends,
    };

    // Provider model cache auto-refresh (only when --webui is active).
    let auto_refresh = matches!(
        std::env::var("PROVIDER_AUTO_REFRESH").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    );
    let refresh_interval_hours: u64 = std::env::var("PROVIDER_REFRESH_INTERVAL_HOURS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(168);

    if auto_refresh {
        let shared_for_refresh = shared.clone();
        let client = crate::main_helpers::providers_cmd::PROVIDER_REFRESH_CLIENT.clone();
        tokio::spawn(async move {
            let interval = std::time::Duration::from_secs(refresh_interval_hours * 3600);
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            loop {
                let providers: Vec<_> = shared_for_refresh
                    .provider_catalog
                    .all_providers()
                    .cloned()
                    .collect();
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
                                let db_ref = shared_for_refresh.db.clone();
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
        tracing::info!(
            interval_hours = refresh_interval_hours,
            "provider auto-refresh enabled"
        );
    }

    // Spawn periodic tasks: log retention and metrics snapshot broadcast.
    let retention_days: u32 = std::env::var("ADMIN_LOG_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(7);

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

    // Periodic metrics snapshot broadcast (every 5 seconds) for WebSocket dashboard.
    let snapshot_shared = shared.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            if snapshot_shared.events_tx.receiver_count() == 0 {
                continue;
            }
            let mut aggregate = anyllm_proxy::metrics::MetricsSnapshot::default();
            for (_, m) in snapshot_shared.backend_metrics.iter() {
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

    // Spawn periodic background health checker with a snapshot of backend base URLs.
    let backend_urls: Vec<(String, String)> = multi_config
        .backends
        .iter()
        .map(|(name, bc)| (name.clone(), bc.base_url.clone()))
        .collect();
    admin::health_check::spawn(shared.clone(), backend_urls);

    // Bind admin listener
    let admin_app = admin::routes::admin_router(shared.clone(), admin_token);
    let admin_addr = format!("{admin_bind}:{admin_port}");
    let admin_listener = tokio::net::TcpListener::bind(&admin_addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind admin to {admin_addr}: {e}"));
    tracing::info!("admin listening on {admin_addr}");

    Some((shared, admin_app, admin_listener, admin_port))
}
