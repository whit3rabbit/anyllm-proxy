pub mod admin;
pub mod oidc;
pub mod otel;
pub mod tools;

use anyllm_proxy::config;
use anyllm_proxy::server::routes;
use std::path::PathBuf;
use std::sync::Arc;

pub async fn async_main(args: Vec<String>, data_dir: PathBuf) {
    // ---- Phase 3: Init tracing (needs RUST_LOG from env file) ----
    let (otel_guard, reload_handle) = otel::init_tracing();
    #[allow(clippy::let_unit_value)]
    let _unused = otel_guard; // Keep guard alive for tracing duration

    // Env aliases were already applied in sync main(). Load config.
    let load_result = config::MultiConfig::load();
    let multi_config = load_result.multi_config;

    #[cfg(not(feature = "secrets-scanner"))]
    if multi_config.redact_secrets {
        panic!("redact_secrets requires building anyllm_proxy with the `secrets-scanner` feature");
    }

    // Ensure a ModelRouter always exists (empty if no config file).
    // Then merge persisted model deployments from SQLite.
    let model_router = {
        let router = load_result.model_router.unwrap_or_else(|| {
            Arc::new(std::sync::RwLock::new(
                config::model_router::ModelRouter::new(std::collections::HashMap::new()),
            ))
        });
        // Load persisted deployments from the DB (best-effort, non-fatal).
        let db_path = super::bootstrap::resolve_db_path(&data_dir);
        if let Ok(conn) = rusqlite::Connection::open(&db_path) {
            if let Ok(rows) = anyllm_proxy::admin::db::list_model_deployments(&conn) {
                if !rows.is_empty() {
                    let mut rw = router.write().unwrap_or_else(|e| e.into_inner());
                    for row in &rows {
                        rw.add_deployment(
                            row.model_name.clone(),
                            Arc::new(config::model_router::Deployment::with_weight(
                                row.backend_name.clone(),
                                row.actual_model.clone(),
                                row.rpm_limit,
                                row.tpm_limit,
                                row.weight,
                            )),
                        );
                    }
                    tracing::info!(
                        count = rows.len(),
                        "loaded persisted model deployments from DB"
                    );
                }
            }
        }
        Some(router)
    };

    // litellm master_key was already applied in fn main() (single-threaded).
    // Log confirmation if it was set.
    if load_result.litellm_master_key.is_some() && std::env::var("PROXY_API_KEYS").is_ok() {
        tracing::info!("general_settings.master_key active as PROXY_API_KEYS");
    }
    let listen_port = multi_config.listen_port;

    // Wire up WEBHOOK_URLS and Langfuse env vars (if not already set from LiteLLM config).
    if anyllm_proxy::server::routes::get_callbacks().is_none() {
        let urls: Vec<String> = std::env::var("WEBHOOK_URLS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let mut named = vec![];
        if let Some(lf) = anyllm_proxy::integrations::LangfuseClient::from_env() {
            tracing::info!("langfuse integration enabled from environment variables");
            named.push(anyllm_proxy::integrations::NamedIntegration::Langfuse(lf));
        }
        if let Some(cb) = anyllm_proxy::callbacks::CallbackConfig::with_named(urls, named) {
            anyllm_proxy::server::routes::set_callbacks(cb);
            tracing::info!("callbacks configured from environment");
        }
    }

    // Warn when request/response body logging is enabled: bodies may contain
    // user prompts, API keys in tool calls, and other sensitive data.
    if multi_config.log_bodies {
        tracing::warn!(
            "LOG_BODIES is enabled: request and response bodies will be logged at debug level. \
             This may expose sensitive data (prompts, API keys, PII). \
             Disable in production by unsetting LOG_BODIES."
        );
    }
    if multi_config.redact_secrets {
        tracing::warn!(
            "REDACT_SECRETS is enabled: upstream JSON/text request payloads will be scanned and \
             detected secrets will be replaced before forwarding."
        );
    }

    tracing::info!(
        backends = ?multi_config.backends.keys().collect::<Vec<_>>(),
        default = %multi_config.default_backend,
        "configured backends"
    );

    // OIDC/JWT authentication setup.
    oidc::init_oidc().await;

    // Warn if infrastructure URLs point at cloud metadata endpoints.
    for var_name in &["QDRANT_URL", "REDIS_URL"] {
        if let Ok(url) = std::env::var(var_name) {
            anyllm_proxy::config::warn_if_cloud_metadata_url(var_name, &url);
        }
    }

    // Redis distributed rate limiting (optional, requires --features redis).
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

    // Initialize Tool Engine State.
    let tool_engine_state = tools::init_tool_engine(load_result.tool_config).await;

    // Initialize Admin UI parts (database, metrics snapshots, health checks, TCP listener).
    let admin_parts = admin::init_admin(
        &args,
        &data_dir,
        &multi_config,
        model_router.clone(),
        &tool_engine_state,
        reload_handle,
    )
    .await;

    let mut admin_redirect_port: Option<u16> = None;
    let mut admin_startup_info: Option<String> = None;
    let enable_admin = admin_parts.is_some();

    if let Some((_, _, _, admin_port)) = &admin_parts {
        admin_redirect_port = Some(*admin_port);
        let admin_bind = std::env::var("ADMIN_BIND").unwrap_or_else(|_| "127.0.0.1".into());
        let admin_display_host = if admin_bind == "0.0.0.0" {
            "localhost"
        } else {
            &admin_bind
        };
        admin_startup_info = Some(format!(
            "http://{}:{}/admin/",
            admin_display_host, admin_port
        ));
    }

    // Initialize batch engine with its own connection to the same DB file.
    // Only available when admin is enabled (requires a DB path).
    let batch_engine: Option<
        std::sync::Arc<
            anyllm_batch_engine::BatchEngine<
                anyllm_batch_engine::queue::sqlite::SqliteQueue,
                anyllm_batch_engine::webhook::sqlite::SqliteWebhookQueue,
            >,
        >,
    > = if enable_admin {
        let db_path = super::bootstrap::resolve_db_path(&data_dir);
        let batch_conn = rusqlite::Connection::open(&db_path)
            .expect("failed to open second SQLite connection for batch engine");
        anyllm_batch_engine::db::migrate_old_tables(&batch_conn)
            .expect("failed to migrate old batch tables");
        anyllm_batch_engine::db::init_batch_engine_tables(&batch_conn)
            .expect("failed to initialize batch engine tables");
        let batch_db = std::sync::Arc::new(std::sync::Mutex::new(batch_conn));
        let global_webhook_urls: Vec<String> = std::env::var("BATCH_WEBHOOK_URLS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let webhook_queue = std::sync::Arc::new(
            anyllm_batch_engine::webhook::sqlite::SqliteWebhookQueue::new(batch_db.clone()),
        );
        let webhook_client =
            anyllm_client::http::build_http_client(&anyllm_client::http::HttpClientConfig {
                ssrf_protection: true,
                connect_timeout: Some(std::time::Duration::from_secs(10)),
                read_timeout: Some(std::time::Duration::from_secs(30)),
                ..Default::default()
            });
        let _webhook_handle = anyllm_batch_engine::webhook::dispatcher::start_dispatcher(
            webhook_queue.clone(),
            webhook_client,
            anyllm_batch_engine::webhook::dispatcher::WebhookConfig::default(),
        );
        Some(std::sync::Arc::new(anyllm_batch_engine::BatchEngine {
            queue: std::sync::Arc::new(anyllm_batch_engine::queue::sqlite::SqliteQueue::new(
                batch_db.clone(),
            )),
            file_store: anyllm_batch_engine::file_store::FileStore::new(batch_db),
            webhook_queue,
            global_webhook_urls,
            webhook_signing_secret: std::env::var("BATCH_WEBHOOK_SIGNING_SECRET").ok(),
        }))
    } else {
        None
    };

    // Build proxy router with optional shared admin state and tool engine.
    let app = routes::app_multi_with_shared(
        multi_config,
        admin_parts.as_ref().map(|(s, _, _, _)| s.clone()),
        model_router,
        tool_engine_state,
        batch_engine,
        admin_redirect_port,
    );

    // --- Start servers ---
    let proxy_addr = format!("0.0.0.0:{listen_port}");
    let proxy_listener = tokio::net::TcpListener::bind(&proxy_addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind proxy to {proxy_addr}: {e}"));
    tracing::info!("proxy listening on {proxy_addr}");

    // Print non-secret startup details once both servers are bound.
    if let Some(admin_url) = &admin_startup_info {
        println!("{}", format_startup_banner(&proxy_addr, admin_url));
    }

    // Warn if API keys are configured and listener is on a non-loopback address.
    let listen_addr = proxy_listener
        .local_addr()
        .unwrap_or_else(|e| panic!("failed to get local address from listener: {e}"));

    let has_proxy_keys = std::env::var("PROXY_API_KEYS").is_ok();
    let has_virtual_keys = admin_parts
        .as_ref()
        .map(|(shared, _, _, _)| !shared.virtual_keys.is_empty())
        .unwrap_or(false);

    if (has_proxy_keys || has_virtual_keys) && !listen_addr.ip().is_loopback() {
        tracing::warn!(
            addr = %listen_addr,
            "proxy is listening on a non-loopback address without TLS; \
             API keys will be transmitted in cleartext. \
             Place a TLS-terminating reverse proxy in front of this service."
        );
    }

    // Warn loudly when open-relay mode is active on a non-loopback address.
    let open_relay_active = std::env::var("PROXY_OPEN_RELAY")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);
    if open_relay_active && !listen_addr.ip().is_loopback() {
        tracing::error!(
            addr = %listen_addr,
            "PROXY_OPEN_RELAY=true on a non-loopback address: any non-empty \
             API key is accepted. This is INSECURE on a publicly reachable address. \
             Use PROXY_API_KEYS or virtual keys instead."
        );
    }

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
        if let Some((_, admin_app, admin_listener, _)) = admin_parts {
            let mut shutdown_rx2 = shutdown_tx.subscribe();
            Some(tokio::spawn(async move {
                axum::serve(
                    admin_listener,
                    admin_app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
                )
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

fn format_startup_banner(proxy_addr: &str, admin_url: &str) -> String {
    let proxy_display = proxy_addr.replace("0.0.0.0", "localhost");
    let border = "─".repeat(56);
    format!("{border}\n  Proxy API  http://{proxy_display}\n  Admin UI   {admin_url}\n{border}")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_banner_does_not_include_admin_token() {
        let token = "sentinel-admin-token-0123456789abcdef";
        let banner = format_startup_banner("0.0.0.0:3000", "http://127.0.0.1:3001/admin/");

        assert!(banner.contains("Proxy API  http://localhost:3000"));
        assert!(banner.contains("Admin UI   http://127.0.0.1:3001/admin/"));
        assert!(!banner.contains(token));
        assert!(!banner.contains("Admin token:"));
        assert!(!banner.contains("Token      "));
    }
}
