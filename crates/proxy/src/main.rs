use anthropic_openai_proxy::{admin, config, server::routes};
use std::sync::Arc;
use tracing_subscriber::prelude::*;

#[tokio::main]
async fn main() {
    // Use a reload layer so the admin API can change log_level at runtime.
    let env_filter = tracing_subscriber::EnvFilter::from_default_env();
    let (filter, reload_handle) = tracing_subscriber::reload::Layer::new(env_filter);
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

    // --- Admin setup ---
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
                "log_level" => runtime_config.log_level = value.clone(),
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

    // Now wrap conn in Arc<Mutex> and start the write buffer.
    let db = Arc::new(tokio::sync::Mutex::new(conn));
    let (events_tx, _) = tokio::sync::broadcast::channel(1024);
    let log_tx = admin::db::spawn_write_buffer(db.clone());

    let backend_metrics: std::collections::HashMap<
        String,
        anthropic_openai_proxy::metrics::Metrics,
    > = std::collections::HashMap::new();

    let shared = admin::state::SharedState {
        db: db.clone(),
        events_tx: events_tx.clone(),
        runtime_config: runtime_config.clone(),
        backend_metrics: Arc::new(backend_metrics),
        log_tx,
        log_reload: Some(log_reload),
    };

    // Admin token: use env var or generate random UUID.
    let admin_token = std::env::var("ADMIN_TOKEN").unwrap_or_else(|_| {
        let token = uuid::Uuid::new_v4().to_string();
        // Print to stderr, not to structured logs (token is a secret).
        eprintln!("Admin token: {token}");
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
            let conn = retention_db.lock().await;
            // Run SQLite IO on the blocking threadpool.
            tokio::task::block_in_place(|| {
                match admin::db::purge_old_logs(&conn, retention_days) {
                    Ok(n) if n > 0 => {
                        tracing::info!(purged = n, "purged old request log entries")
                    }
                    Err(e) => tracing::error!(error = %e, "failed to purge old logs"),
                    _ => {}
                }
            });
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
            let mut aggregate = anthropic_openai_proxy::metrics::MetricsSnapshot::default();
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

    // Build proxy router with shared state.
    let app = routes::app_multi_with_shared(multi_config, Some(shared.clone()));

    // Build admin router.
    let admin_app = admin::routes::admin_router(shared, admin_token);

    // --- Start both servers ---
    let proxy_addr = format!("0.0.0.0:{listen_port}");
    let proxy_listener = tokio::net::TcpListener::bind(&proxy_addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind proxy to {proxy_addr}: {e}"));
    tracing::info!("proxy listening on {proxy_addr}");

    let admin_addr = format!("127.0.0.1:{admin_port}");
    let admin_listener = tokio::net::TcpListener::bind(&admin_addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind admin to {admin_addr}: {e}"));
    tracing::info!("admin listening on {admin_addr}");

    // Share the shutdown signal between both servers via a tokio::sync::watch.
    let (shutdown_tx, mut shutdown_rx1) = tokio::sync::watch::channel(false);
    let mut shutdown_rx2 = shutdown_tx.subscribe();

    // Spawn the proxy server.
    let proxy_handle = tokio::spawn(async move {
        axum::serve(proxy_listener, app)
            .with_graceful_shutdown(async move {
                shutdown_rx1.changed().await.ok();
            })
            .await
            .expect("proxy server error");
    });

    // Spawn the admin server.
    let admin_handle = tokio::spawn(async move {
        axum::serve(admin_listener, admin_app)
            .with_graceful_shutdown(async move {
                shutdown_rx2.changed().await.ok();
            })
            .await
            .expect("admin server error");
    });

    // Wait for shutdown signal, then notify both servers.
    shutdown_signal().await;
    let _ = shutdown_tx.send(true);

    // Wait for both servers to finish.
    let _ = tokio::join!(proxy_handle, admin_handle);
    tracing::info!("server shut down gracefully");
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
