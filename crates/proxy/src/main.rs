use anyllm_proxy::{admin, config, server::routes};
use std::sync::Arc;
use tracing_subscriber::prelude::*;

#[tokio::main]
async fn main() {
    // ---- Phase 1: Collect env file overrides (before tracing init) ----
    let args: Vec<String> = std::env::args().collect();
    let env_file_path = args
        .windows(2)
        .find(|w| w[0] == "--env-file")
        .map(|w| w[1].as_str())
        .or_else(|| {
            if std::path::Path::new(".anyllm.env").exists() {
                Some(".anyllm.env")
            } else {
                None
            }
        });
    let env_file_vars = env_file_path.map(parse_env_file).unwrap_or_default();

    // ---- Phase 2: Apply env file vars (needed for RUST_LOG before tracing init) ----
    // SAFETY: single-threaded, before tokio spawns workers.
    unsafe {
        for (key, val) in &env_file_vars {
            std::env::set_var(key, val);
        }
    }
    if !env_file_vars.is_empty() {
        eprintln!(
            "anyllm_proxy: loaded {} variable(s) from env file",
            env_file_vars.len()
        );
    }

    // ---- Phase 3: Init tracing (needs RUST_LOG from env file) ----
    let env_filter = tracing_subscriber::EnvFilter::from_default_env();
    let (filter, reload_handle) = tracing_subscriber::reload::Layer::new(env_filter);

    #[cfg(feature = "otel")]
    let _otel_guard = {
        let (guard, tracer) = anyllm_proxy::otel::init_otel();
        let otel_layer = tracing_opentelemetry::OpenTelemetryLayer::new(tracer);
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().json())
            .with(otel_layer)
            .init();
        guard
    };

    #[cfg(not(feature = "otel"))]
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    // ---- Phase 4: Compute remaining env overrides and apply in one block ----
    let aliases = config::env_aliases::compute_env_aliases();

    // Apply alias overrides so config::MultiConfig::load() sees them.
    // SAFETY: still single-threaded at this point (no spawns yet).
    unsafe {
        for (key, val) in &aliases {
            std::env::set_var(key, val);
        }
    }

    let load_result = config::MultiConfig::load();
    let multi_config = load_result.multi_config;
    let model_router = load_result.model_router;

    // Apply litellm master_key if PROXY_API_KEYS is still unset.
    if let Some(ref mk) = load_result.litellm_master_key {
        if std::env::var("PROXY_API_KEYS").is_err() {
            // SAFETY: still single-threaded, no spawns yet.
            unsafe { std::env::set_var("PROXY_API_KEYS", mk) };
            tracing::info!("applied general_settings.master_key as PROXY_API_KEYS");
        }
    }
    let listen_port = multi_config.listen_port;

    // Wire up WEBHOOK_URLS env var (if LiteLLM callbacks weren't already set).
    if let Some(cb) = anyllm_proxy::callbacks::CallbackConfig::from_env() {
        anyllm_proxy::server::routes::set_callbacks(cb);
        tracing::info!("webhook callbacks configured from WEBHOOK_URLS env var");
    }

    tracing::info!(
        backends = ?multi_config.backends.keys().collect::<Vec<_>>(),
        default = %multi_config.default_backend,
        "configured backends"
    );

    // OIDC/JWT authentication (optional). When OIDC_ISSUER_URL is set, discover
    // the OIDC configuration and load JWKS. Tokens that look like JWTs are
    // validated against the JWKS before falling through to key-based auth.
    if let Ok(issuer_url) = std::env::var("OIDC_ISSUER_URL") {
        let audience = std::env::var("OIDC_AUDIENCE").unwrap_or_else(|_| {
            tracing::warn!(
                "OIDC_ISSUER_URL is set but OIDC_AUDIENCE is not; using issuer URL as audience"
            );
            issuer_url.clone()
        });
        match anyllm_proxy::server::oidc::OidcConfig::discover(&issuer_url, &audience).await {
            Ok(config) => {
                let config = Arc::new(config);
                anyllm_proxy::server::middleware::set_oidc_config(config.clone());
                // Background task: refresh JWKS every 60 minutes.
                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
                    interval.tick().await; // skip immediate tick
                    loop {
                        interval.tick().await;
                        if let Err(e) = config.refresh_jwks().await {
                            tracing::warn!("JWKS refresh failed: {e}");
                        } else {
                            tracing::debug!("JWKS refreshed successfully");
                        }
                    }
                });
                tracing::info!(issuer = %issuer_url, "OIDC/JWT authentication enabled");
            }
            Err(e) => {
                tracing::error!("OIDC discovery failed: {e}. Starting without OIDC auth.");
            }
        }
    }

    // Redis distributed rate limiting (optional, requires --features redis).
    // When REDIS_URL is set, RPM/TPM checks are performed against Redis so
    // multiple proxy instances share rate limit state.
    #[cfg(feature = "redis")]
    if let Ok(redis_url) = std::env::var("REDIS_URL") {
        let fail_policy = anyllm_proxy::ratelimit::RateLimitFailPolicy::from_env();
        match anyllm_proxy::ratelimit::RedisRateLimiter::new(&redis_url, fail_policy).await {
            Ok(limiter) => {
                anyllm_proxy::ratelimit::set_redis_rate_limiter(limiter);
                tracing::info!(?fail_policy, "Redis distributed rate limiting enabled");
            }
            Err(e) => {
                tracing::error!("Redis connection failed: {e}. Using local-only rate limiting.");
            }
        }
    }

    // Admin web UI is opt-in: pass --webui or --admin to enable.
    // DISABLE_ADMIN=1 overrides the flag (useful in container/scripted environments).
    let flag_set = args.iter().any(|a| a == "--webui" || a == "--admin");
    let force_disabled = matches!(
        std::env::var("DISABLE_ADMIN").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    );
    let enable_admin = flag_set && !force_disabled;

    // --- Admin setup (enabled only when --webui or --admin flag is passed) ---
    // Returns Some((SharedState, admin Router, admin TcpListener)) when enabled.
    let admin_parts = if enable_admin {
        let admin_port: u16 = std::env::var("ADMIN_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3001);

        if admin_port == listen_port {
            panic!("ADMIN_PORT ({admin_port}) must differ from LISTEN_PORT ({listen_port})");
        }

        // SQLite: open or create the database file in the current directory.
        let db_path = std::env::var("ADMIN_DB_PATH").unwrap_or_else(|_| "admin.db".into());
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
        };

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
                    "log_bodies" => runtime_config.log_bodies = value == "true",
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
        let runtime_config = Arc::new(std::sync::RwLock::new(runtime_config));

        // Build the log_reload closure that captures the reload handle.
        let log_reload: Arc<dyn Fn(&str) -> bool + Send + Sync> = {
            let handle = reload_handle;
            Arc::new(move |new_filter: &str| {
                match tracing_subscriber::EnvFilter::try_new(new_filter) {
                    Ok(f) => handle.reload(f).is_ok(),
                    Err(e) => {
                        tracing::error!(filter = new_filter, error = %e, "invalid log filter string");
                        false
                    }
                }
            })
        };

        // Now wrap conn in Arc<Mutex> and start the write buffer.
        // Uses std::sync::Mutex because rusqlite is synchronous; all access
        // goes through spawn_blocking to avoid stalling the tokio executor.
        let db = Arc::new(std::sync::Mutex::new(conn));
        let (events_tx, _) = tokio::sync::broadcast::channel(1024);
        let log_tx = admin::db::spawn_write_buffer(db.clone());

        let backend_metrics: std::collections::HashMap<String, anyllm_proxy::metrics::Metrics> =
            std::collections::HashMap::new();

        // Load active virtual keys from SQLite into in-memory DashMap.
        let virtual_keys = Arc::new(dashmap::DashMap::new());
        {
            let conn_guard = db.lock().unwrap_or_else(|e| e.into_inner());
            if let Ok(active_keys) = admin::db::load_active_virtual_keys(&conn_guard) {
                for key_row in &active_keys {
                    if let Some(hash_bytes) = admin::keys::hash_from_hex(&key_row.key_hash) {
                        virtual_keys.insert(
                            hash_bytes,
                            admin::keys::VirtualKeyMeta {
                                id: key_row.id,
                                description: key_row.description.clone(),
                                expires_at: None,
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
                // Check and prune old rate limit states
                for entry in virtual_keys_pruner.iter() {
                    let _ = entry.rate_state.check_rpm(0, now);
                    let _ = entry.rate_state.check_tpm(0, now);
                }
            }
        });

        let shared = admin::state::SharedState {
            db: db.clone(),
            events_tx: events_tx.clone(),
            runtime_config: runtime_config.clone(),
            backend_metrics: Arc::new(backend_metrics),
            log_tx,
            log_reload: Some(log_reload),
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            virtual_keys,
            hmac_secret,
            model_router: model_router.clone(),
        };

        // Admin token: use env var or generate random UUID written to a file.
        let admin_token = std::env::var("ADMIN_TOKEN").unwrap_or_else(|_| {
            let token = uuid::Uuid::new_v4().to_string();
            let token_path = std::env::var("ADMIN_TOKEN_FILE")
                .unwrap_or_else(|_| ".admin_token".into());
            // Write token to file with restrictive permissions instead of stderr,
            // because stderr is captured by container log drivers in production.
            if let Err(e) = write_token_file(&token_path, &token) {
                // Do not print the token to stderr: container log drivers capture
                // stderr and persist it in centralized logging systems.
                panic!(
                    "Cannot write admin token to {token_path}: {e}. \
                     Set ADMIN_TOKEN env var explicitly or ensure the path is writable."
                );
            } else {
                // Log the path, not the token itself.
                tracing::info!(path = %token_path, "generated admin token written to file (set ADMIN_TOKEN env var to avoid this)");
            }
            token
        });
        let admin_token = Arc::new(admin_token);

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
                admin::state::with_db(&retention_db, move |conn| {
                    match admin::db::purge_old_logs(conn, retention_days) {
                        Ok(n) if n > 0 => {
                            tracing::info!(purged = n, "purged old request log entries")
                        }
                        Err(e) => tracing::error!(error = %e, "failed to purge old logs"),
                        _ => {}
                    }
                })
                .await;
            }
        });

        // Periodic metrics snapshot broadcast (every 5 seconds) for WebSocket dashboard.
        let snapshot_shared = shared.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
            loop {
                interval.tick().await;
                // Skip computation if no WebSocket clients are listening.
                if snapshot_shared.events_tx.receiver_count() == 0 {
                    continue;
                }
                let mut backends = std::collections::HashMap::new();
                let mut aggregate = anyllm_proxy::metrics::MetricsSnapshot::default();
                for (name, m) in snapshot_shared.backend_metrics.iter() {
                    let snap = m.snapshot();
                    aggregate.requests_total += snap.requests_total;
                    aggregate.requests_error += snap.requests_error;
                    aggregate.requests_success += snap.requests_success;
                    backends.insert(name.clone(), snap);
                }
                let error_rate = aggregate.error_rate();
                let snapshot = admin::state::MetricsSnapshotData {
                    backends,
                    latency_p50_ms: None, // Computed on demand by REST endpoint
                    latency_p95_ms: None,
                    latency_p99_ms: None,
                    requests_per_second: 0.0, // TODO: compute from recent request log
                    error_rate,
                };
                let _ = snapshot_shared
                    .events_tx
                    .send(admin::state::AdminEvent::MetricsSnapshot(snapshot));
            }
        });

        // Bind admin listener; spawned after the shutdown channel is created below.
        let admin_app = admin::routes::admin_router(shared.clone(), admin_token);
        let admin_addr = format!("127.0.0.1:{admin_port}");
        let admin_listener = tokio::net::TcpListener::bind(&admin_addr)
            .await
            .unwrap_or_else(|e| panic!("failed to bind admin to {admin_addr}: {e}"));
        tracing::info!("admin listening on {admin_addr}");

        Some((shared, admin_app, admin_listener))
    } else {
        None
    };

    // Build proxy router with optional shared admin state.
    let app = routes::app_multi_with_shared(
        multi_config,
        admin_parts.as_ref().map(|(s, _, _)| s.clone()),
        model_router,
    );

    // --- Start servers ---
    let proxy_addr = format!("0.0.0.0:{listen_port}");
    let proxy_listener = tokio::net::TcpListener::bind(&proxy_addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind proxy to {proxy_addr}: {e}"));
    tracing::info!("proxy listening on {proxy_addr}");

    // Single shutdown channel shared by proxy and (optionally) admin.
    let (shutdown_tx, mut shutdown_rx1) = tokio::sync::watch::channel(false);

    let proxy_handle = tokio::spawn(async move {
        axum::serve(proxy_listener, app)
            .with_graceful_shutdown(async move {
                shutdown_rx1.changed().await.ok();
            })
            .await
            .expect("proxy server error");
    });

    let admin_handle: Option<tokio::task::JoinHandle<()>> =
        if let Some((_, admin_app, admin_listener)) = admin_parts {
            let mut shutdown_rx2 = shutdown_tx.subscribe();
            Some(tokio::spawn(async move {
                axum::serve(admin_listener, admin_app)
                    .with_graceful_shutdown(async move {
                        shutdown_rx2.changed().await.ok();
                    })
                    .await
                    .expect("admin server error");
            }))
        } else {
            None
        };

    shutdown_signal().await;
    let _ = shutdown_tx.send(true);

    let _ = proxy_handle.await;
    if let Some(h) = admin_handle {
        let _ = h.await;
    }
    tracing::info!("server shut down gracefully");
}

/// Parse a `.env`-format file and return `(key, value)` pairs to set.
///
/// Rules:
/// - `KEY=VALUE` sets the variable. Surrounding whitespace is trimmed.
/// - Values may be optionally wrapped in `"double"` or `'single'` quotes.
/// - Lines starting with `#` (after trimming) are comments.
/// - Already-set environment variables are skipped; the real
///   environment always takes precedence over the file.
/// - `export KEY=VALUE` syntax is supported (the `export` prefix is stripped).
///
/// Returns pairs that should be applied via `set_var` in the consolidated block.
/// Compatible with Docker `--env-file` and standard dotenv tooling.
fn parse_env_file(path: &str) -> Vec<(String, String)> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            // Print directly; tracing isn't initialized yet.
            eprintln!("anyllm_proxy: could not read env file '{path}': {e}");
            return Vec::new();
        }
    };

    let mut pairs = Vec::new();
    for (lineno, raw) in content.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Strip optional `export ` prefix.
        let line = line.strip_prefix("export ").map(str::trim).unwrap_or(line);
        let Some((key, val)) = line.split_once('=') else {
            eprintln!(
                "anyllm_proxy: {path}:{}: ignoring malformed line (no '=')",
                lineno + 1
            );
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        // Strip optional surrounding quotes from the value.
        let val = val.trim();
        let val = if (val.starts_with('"') && val.ends_with('"'))
            || (val.starts_with('\'') && val.ends_with('\''))
        {
            &val[1..val.len() - 1]
        } else {
            val
        };
        // Only include if not already present so the real environment wins.
        if std::env::var(key).is_err() {
            pairs.push((key.to_string(), val.to_string()));
        }
    }
    pairs
}

/// Write the admin token to a file with mode 0600 (owner-only read/write).
/// On Unix, sets permissions atomically at creation to avoid a TOCTOU race
/// where the file is briefly world-readable before chmod.
fn write_token_file(path: &str, token: &str) -> std::io::Result<()> {
    use std::io::Write;

    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?
    };

    #[cfg(not(unix))]
    let mut file = {
        tracing::warn!(
            path = %path,
            "non-Unix platform: admin token file may be world-readable. \
             Set ADMIN_TOKEN env var explicitly in production."
        );
        std::fs::File::create(path)?
    };

    file.write_all(token.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("failed to install SIGTERM handler");

    #[cfg(unix)]
    tokio::select! {
        _ = ctrl_c => { tracing::info!("received SIGINT, starting graceful shutdown"); }
        _ = sigterm.recv() => { tracing::info!("received SIGTERM, starting graceful shutdown"); }
    }

    #[cfg(not(unix))]
    {
        ctrl_c.await.expect("failed to listen for Ctrl+C");
        tracing::info!("received Ctrl+C, starting graceful shutdown");
    }
}
