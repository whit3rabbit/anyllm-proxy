use anyllm_proxy::{admin, config, server::routes};
use std::sync::Arc;
use tracing_subscriber::prelude::*;

#[tokio::main]
async fn main() {
    // Load env file before anything else so RUST_LOG and backend config are visible.
    // Explicit --env-file <path> takes priority; otherwise auto-load .anyllm.env if present.
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
    if let Some(path) = env_file_path {
        load_env_file(path);
    }

    // Use a reload layer so the admin API can change log_level at runtime.
    let env_filter = tracing_subscriber::EnvFilter::from_default_env();
    let (filter, reload_handle) = tracing_subscriber::reload::Layer::new(env_filter);

    // When the `otel` feature is enabled, wire an OpenTelemetry tracing layer
    // into the subscriber so that spans are exported as OTLP traces.
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

    let multi_config = config::MultiConfig::load();
    let listen_port = multi_config.listen_port;

    tracing::info!(
        backends = ?multi_config.backends.keys().collect::<Vec<_>>(),
        default = %multi_config.default_backend,
        "configured backends"
    );

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
                                expires_at: None, // Expiry checked from the ISO string at auth time
                                rpm_limit: key_row.rpm_limit,
                                tpm_limit: key_row.tpm_limit,
                                rate_state: Arc::new(admin::keys::RateLimitState::new()),
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

        // Make virtual keys available to the auth middleware.
        anyllm_proxy::server::middleware::set_virtual_keys(virtual_keys.clone());

        let shared = admin::state::SharedState {
            db: db.clone(),
            events_tx: events_tx.clone(),
            runtime_config: runtime_config.clone(),
            backend_metrics: Arc::new(backend_metrics),
            log_tx,
            log_reload: Some(log_reload),
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            virtual_keys,
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

/// Load a `.env`-format file and apply values to the process environment.
///
/// Rules:
/// - `KEY=VALUE` sets the variable. Surrounding whitespace is trimmed.
/// - Values may be optionally wrapped in `"double"` or `'single'` quotes.
/// - Lines starting with `#` (after trimming) are comments.
/// - Already-set environment variables are never overwritten; the real
///   environment always takes precedence over the file.
/// - `export KEY=VALUE` syntax is supported (the `export` prefix is stripped).
///
/// Compatible with Docker `--env-file` and standard dotenv tooling.
fn load_env_file(path: &str) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            // Print directly; tracing isn't initialized yet.
            eprintln!("anyllm_proxy: could not read env file '{path}': {e}");
            return;
        }
    };

    let mut loaded = 0usize;
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
        // Only set if not already present so the real environment wins.
        if std::env::var(key).is_err() {
            // SAFETY: called before any threads are spawned (before tokio runtime).
            #[allow(deprecated)]
            std::env::set_var(key, val);
            loaded += 1;
        }
    }
    eprintln!("anyllm_proxy: loaded {loaded} variable(s) from '{path}'");
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
