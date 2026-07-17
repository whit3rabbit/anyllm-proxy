pub(crate) mod config;
pub(crate) mod tasks;

use anyllm_proxy::admin;
use anyllm_proxy::config as proxy_config;
use anyllm_proxy::server::state as server_state;
use std::path::Path;
use std::sync::Arc;

#[allow(clippy::too_many_lines)]
pub(crate) async fn init_admin(
    args: &[String],
    data_dir: &Path,
    multi_config: &proxy_config::MultiConfig,
    model_router: Option<Arc<std::sync::RwLock<proxy_config::model_router::ModelRouter>>>,
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
    String,
)> {
    if !crate::main_helpers::bootstrap::admin_enabled(args) {
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
        match raw.to_ascii_lowercase().as_str() {
            "localhost" => "127.0.0.1".to_string(),
            "localhost6" | "ip6-localhost" | "ip6-loopback" => "::1".to_string(),
            _ => raw,
        }
    };
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

    // Load configuration and apply database overrides
    let (mut runtime_config, runtime_defaults) =
        config::load_runtime_config(multi_config, tool_engine_state);
    let (log_bodies_override, redact_secrets_override) =
        config::apply_config_overrides(&conn, &mut runtime_config, multi_config);

    if log_bodies_override {
        tracing::warn!(
            "persisted admin override enabled LOG_BODIES: request and response bodies will be \
             logged at debug level. This may expose sensitive data (prompts, API keys, PII)."
        );
    }
    if redact_secrets_override {
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

    // Load virtual API keys and setup rate-limit state
    let virtual_keys = config::load_virtual_keys(&conn);
    anyllm_proxy::server::middleware::set_virtual_keys(virtual_keys.clone());
    anyllm_proxy::server::middleware::set_hmac_secret(hmac_secret.clone());

    // Spawn key pruner background task
    tasks::spawn_background_pruner(virtual_keys.clone());

    // Load managed backends
    let managed_backends = config::load_managed_backends(&conn, &provider_catalog);

    let db = Arc::new(std::sync::Mutex::new(conn));

    // Compile initial route dispatch table
    let route_router = {
        let conn_guard = db.lock().unwrap_or_else(|e| e.into_inner());
        let rr = proxy_config::route_router::RouteRouter::build_from_db(&conn_guard)
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "failed to build route router from DB; starting empty");
                proxy_config::route_router::RouteRouter::empty()
            });
        tracing::info!(
            has_routes = !rr.is_empty(),
            "initialized route dispatch table"
        );
        Some(Arc::new(std::sync::RwLock::new(rr)))
    };

    let (events_tx, _) = tokio::sync::broadcast::channel(1024);
    let log_tx = admin::db::spawn_write_buffer(db.clone());
    let backend_metrics = std::collections::HashMap::new();

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

    // Resolve admin token
    let (admin_token_plain, admin_token) = config::resolve_admin_token(data_dir);

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
        route_router,
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
        listen_port: multi_config.listen_port,
        managed_backends,
        // Mirror when `all_backends` is populated in routes.rs (only under an
        // active model router). Lets the router-config PUT validator accept tiers
        // targeting statically-configured backends, not just managed ones.
        static_backends: Arc::new(if model_router.is_some() {
            multi_config.backends.keys().cloned().collect()
        } else {
            std::collections::HashSet::new()
        }),
    };

    // Provider model cache auto-refresh (only when --webui is active)
    let auto_refresh = matches!(
        std::env::var("PROVIDER_AUTO_REFRESH").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    );
    let refresh_interval_hours: u64 = std::env::var("PROVIDER_REFRESH_INTERVAL_HOURS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(168);

    if auto_refresh {
        tasks::spawn_auto_refresh_task(shared.clone(), refresh_interval_hours);
        tracing::info!(
            interval_hours = refresh_interval_hours,
            "provider auto-refresh enabled"
        );
    }

    // Periodic tasks: log retention and metrics snapshot
    let retention_days: u32 = std::env::var("ADMIN_LOG_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(7);
    tasks::spawn_periodic_tasks(shared.clone(), retention_days);

    // Spawn periodic background health checker
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
        .unwrap_or_else(|e| {
            eprintln!(
                "error: failed to bind admin server to {admin_addr}: {e}\n\
                 The port may already be in use. You can choose a different admin port by setting the ADMIN_PORT environment variable (e.g. `ADMIN_PORT=3002 anyllm-proxy`)."
            );
            std::process::exit(1);
        });
    tracing::info!("admin listening on {admin_addr}");

    Some((
        shared,
        admin_app,
        admin_listener,
        admin_port,
        admin_token_plain,
    ))
}
