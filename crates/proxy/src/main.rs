use anyllm_proxy::{
    admin, config,
    server::{routes, state},
    tools,
};
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::prelude::*;

/// Entry point: set env vars while still single-threaded (before tokio runtime),
/// then hand off to the async main. This avoids UB from calling set_var after
/// the multi-thread tokio runtime has spawned worker threads.
fn main() {
    let args: Vec<String> = std::env::args().collect();

    // When spawned as a proxy child by the "run" subcommand, env vars are already
    // inherited from the parent process; skip env file loading to avoid duplicate messages.
    let is_run_child = std::env::var("_ANYLLM_RUN_CHILD").is_ok();

    // Resolve the data directory early so all path defaults can use it.
    let data_dir = resolve_data_dir();
    if !is_run_child {
        eprintln!("anyllm_proxy: data directory: {}", data_dir.display());
        let data_dir_env = data_dir.join(".anyllm.env");
        let env_file_path = args
            .windows(2)
            .find(|w| w[0] == "--env-file")
            .map(|w| w[1].to_string())
            .or_else(|| {
                if std::path::Path::new(".anyllm.env").exists() {
                    Some(".anyllm.env".into())
                } else if data_dir_env.exists() {
                    Some(data_dir_env.to_string_lossy().into_owned())
                } else {
                    None
                }
            });
        let env_file_vars = env_file_path
            .as_deref()
            .map(parse_env_file)
            .unwrap_or_default();

        // SAFETY: genuinely single-threaded here (no tokio runtime yet).
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
    }

    // Compute and apply LiteLLM env aliases (still single-threaded).
    let aliases = config::env_aliases::compute_env_aliases();
    unsafe {
        for (key, val) in &aliases {
            std::env::set_var(key, val);
        }
    }

    // Load any env vars previously imported via the admin UI (persisted in SQLite).
    // Runs after .anyllm.env so the file still takes precedence over DB imports,
    // and before the async runtime to keep set_var single-threaded safe.
    if !is_run_child {
        let db_path = resolve_db_path(&data_dir);
        let db_vars = load_env_from_sqlite(&db_path);
        if !db_vars.is_empty() {
            unsafe {
                for (key, val) in &db_vars {
                    std::env::set_var(key, val);
                }
            }
            eprintln!(
                "anyllm_proxy: applied {} variable(s) from admin DB env import",
                db_vars.len()
            );
        }
    }

    // Auto-detect config file in data directory if PROXY_CONFIG is not set.
    if std::env::var("PROXY_CONFIG").is_err() {
        let data_config = data_dir.join("config.yaml");
        if data_config.exists() {
            let path_str = data_config.to_string_lossy().into_owned();
            unsafe { std::env::set_var("PROXY_CONFIG", &path_str) };
            eprintln!("anyllm_proxy: auto-detected config: {path_str}");
        }
    }

    // Extract litellm master_key before the runtime starts (still single-threaded).
    if std::env::var("PROXY_API_KEYS").is_err() {
        if let Ok(ref config_path) = std::env::var("PROXY_CONFIG") {
            if let Some(mk) = config::extract_litellm_master_key(config_path) {
                // SAFETY: genuinely single-threaded here (no tokio runtime yet).
                unsafe { std::env::set_var("PROXY_API_KEYS", &mk) };
                eprintln!("anyllm_proxy: applied general_settings.master_key as PROXY_API_KEYS");
            }
        }
    }

    // Warn when no backend is configured so users aren't left guessing why
    // requests fail. Skip when spawned as a child of the "run" subcommand.
    if !is_run_child && !anyllm_proxy::admin::routes::status::is_backend_configured() {
        eprintln!(
            "\n\
anyllm-proxy: no backend configured. The proxy has nothing to forward requests to.\n\
\n\
The proxy needs an endpoint to forward to (backend) and a port to listen on (front).\n\
LISTEN_PORT defaults to 3000. Pick a backend:\n\
\n\
  # OpenAI (remote, needs API key)\n\
  OPENAI_API_KEY=sk-...\n\
  PROXY_API_KEYS=my-key          # key your clients send\n\
\n\
  # Ollama / local LLM (no API key required)\n\
  OPENAI_BASE_URL=http://localhost:11434/v1\n\
  PROXY_OPEN_RELAY=true          # allow any key (local dev only)\n\
\n\
  # OpenRouter / any OpenAI-compatible endpoint\n\
  OPENAI_BASE_URL=https://openrouter.ai/api/v1\n\
  OPENAI_API_KEY=sk-or-...\n\
  PROXY_API_KEYS=my-key\n\
\n\
Save to ~/.anyllm/.anyllm.env or load explicitly:\n\
\n\
  anyllm-proxy --env-file /path/to/.anyllm.env\n\
\n\
Configure via UI:  anyllm-proxy --webui\n"
        );
    }

    // Detect "run" subcommand: anyllm_proxy [proxy_opts...] run <command> [args...]
    // Starts the proxy in the background and launches <command> with the proxy's
    // ANTHROPIC_* env vars pre-configured, then exits when <command> exits.
    if let Some(run_idx) = args.iter().position(|a| a == "run") {
        let tool_argv: Vec<String> = args[run_idx + 1..].to_vec();
        if tool_argv.is_empty() {
            eprintln!("usage: anyllm_proxy [--env-file FILE] run <command> [args...]");
            std::process::exit(1);
        }
        // Proxy args: everything between the binary name and "run".
        let proxy_args: Vec<String> = args[1..run_idx].to_vec();
        std::process::exit(run_subcommand(proxy_args, tool_argv));
    }

    // Detect "providers" subcommand: anyllm-proxy providers list [--json]
    //                                anyllm-proxy providers refresh <id>
    //                                anyllm-proxy providers refresh --all
    if let Some(pos) = args.iter().position(|a| a == "providers") {
        let subcmd_args: Vec<String> = args[pos + 1..].to_vec();
        std::process::exit(providers_subcommand(subcmd_args, &data_dir));
    }

    // Now start the tokio runtime and enter the async main.
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime")
        .block_on(async_main(args, data_dir));
}

use std::sync::LazyLock;

/// Shared HTTP client for provider model discovery (connect 10s, read 20s, no redirects).
static PROVIDER_REFRESH_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("failed to build provider refresh HTTP client")
});

fn provider_status_str(s: anyllm_providers::provider::ProviderStatus) -> &'static str {
    match s {
        anyllm_providers::provider::ProviderStatus::Implemented => "implemented",
        anyllm_providers::provider::ProviderStatus::Wired => "wired",
        anyllm_providers::provider::ProviderStatus::Stub => "stub",
    }
}

fn provider_protocol_str(p: anyllm_providers::provider::ProviderProtocol) -> &'static str {
    match p {
        anyllm_providers::provider::ProviderProtocol::OpenAICompat => "openai_compat",
        anyllm_providers::provider::ProviderProtocol::AzureOpenAI => "azure_openai",
        anyllm_providers::provider::ProviderProtocol::VertexAI => "vertex_ai",
        anyllm_providers::provider::ProviderProtocol::GeminiOpenAI => "gemini_openai",
        anyllm_providers::provider::ProviderProtocol::GeminiNative => "gemini_native",
        anyllm_providers::provider::ProviderProtocol::AnthropicNative => "anthropic_native",
        anyllm_providers::provider::ProviderProtocol::BedrockNative => "bedrock_native",
        anyllm_providers::provider::ProviderProtocol::Custom => "custom",
    }
}

/// CLI handler for `anyllm-proxy providers …`.
/// Runs synchronously (blocking HTTP calls via a throw-away tokio runtime).
/// Does not write to SQLite — the HTTP fetch is informational only.
fn providers_subcommand(args: Vec<String>, _data_dir: &std::path::Path) -> i32 {
    match args.first().map(String::as_str) {
        Some("list") => {
            let json_mode = args.iter().any(|a| a == "--json");
            let providers: Vec<_> = anyllm_providers::all_providers().collect();
            if json_mode {
                let out: Vec<serde_json::Value> = providers
                    .iter()
                    .map(|p| {
                        serde_json::json!({
                            "id":               p.id,
                            "display_name":     p.display_name,
                            "status":           provider_status_str(p.status),
                            "protocol":         provider_protocol_str(p.protocol),
                            "chat_completions": p.capabilities.chat_completions,
                            "model_count":      anyllm_providers::list_models(p.id).len(),
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&out).unwrap());
            } else {
                println!("{:<20} {:<15} {:<12} {:>8}", "ID", "STATUS", "PROTOCOL", "MODELS");
                println!("{}", "-".repeat(60));
                for p in &providers {
                    println!(
                        "{:<20} {:<15} {:<12} {:>8}",
                        p.id,
                        provider_status_str(p.status),
                        provider_protocol_str(p.protocol),
                        anyllm_providers::list_models(p.id).len()
                    );
                }
            }
            0
        }
        Some("refresh") => {
            let target = args.get(1).map(String::as_str);
            let refresh_all = target == Some("--all");

            // Collect providers to refresh before entering async context.
            let providers_to_refresh: Vec<anyllm_providers::provider::ProviderDef> =
                if refresh_all {
                    anyllm_providers::all_providers()
                        .filter(|p| p.capabilities.chat_completions)
                        .filter(|p| p.env_vars.iter().any(|v| std::env::var(v).is_ok()))
                        .cloned()
                        .collect()
                } else if let Some(id) = target {
                    match anyllm_providers::get_provider(id) {
                        Some(p) => vec![p.clone()],
                        None => {
                            eprintln!("error: unknown provider '{id}'");
                            return 1;
                        }
                    }
                } else {
                    eprintln!("usage: anyllm-proxy providers refresh <provider-id>|--all");
                    return 1;
                };

            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build runtime");

            let client = PROVIDER_REFRESH_CLIENT.clone();

            let mut exit = 0;
            for provider in &providers_to_refresh {
                let api_key = provider.env_vars.iter().find_map(|v| std::env::var(v).ok());
                let url = format!("{}/v1/models", provider.default_base_url.trim_end_matches('/'));
                let result = rt.block_on(async {
                    let mut req = client.get(&url);
                    if let Some(ref key) = api_key {
                        req = req.header("Authorization", format!("Bearer {key}"));
                    }
                    req.send().await
                });
                match result {
                    Err(e) => {
                        eprintln!("{}: error: {e}", provider.id);
                        exit = 1;
                    }
                    Ok(resp) if !resp.status().is_success() => {
                        eprintln!("{}: upstream returned {}", provider.id, resp.status());
                        exit = 1;
                    }
                    Ok(resp) => match rt.block_on(resp.json::<serde_json::Value>()) {
                        Err(e) => {
                            eprintln!("{}: invalid JSON: {e}", provider.id);
                            exit = 1;
                        }
                        Ok(json) => {
                            let models: Vec<&str> = json
                                .get("data")
                                .and_then(|d| d.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|m| m.get("id")?.as_str())
                                        .collect()
                                })
                                .unwrap_or_default();
                            println!("{}: {} models", provider.id, models.len());
                            for m in &models {
                                println!("  - {m}");
                            }
                        }
                    },
                }
            }
            exit
        }
        _ => {
            eprintln!("usage: anyllm-proxy providers list [--json]");
            eprintln!("       anyllm-proxy providers refresh <provider-id>");
            eprintln!("       anyllm-proxy providers refresh --all");
            1
        }
    }
}

async fn async_main(args: Vec<String>, data_dir: PathBuf) {
    // ---- Phase 3: Init tracing (needs RUST_LOG from env file) ----
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
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

    // Env aliases were already applied in sync main(). Load config.
    let load_result = config::MultiConfig::load();
    let multi_config = load_result.multi_config;

    // Ensure a ModelRouter always exists (empty if no config file).
    // Then merge persisted model deployments from SQLite.
    let model_router = {
        let router = load_result.model_router.unwrap_or_else(|| {
            Arc::new(std::sync::RwLock::new(
                config::model_router::ModelRouter::new(std::collections::HashMap::new()),
            ))
        });
        // Load persisted deployments from the DB (best-effort, non-fatal).
        let db_path = resolve_db_path(&data_dir);
        if let Ok(conn) = rusqlite::Connection::open(&db_path) {
            if let Ok(rows) = admin::db::list_model_deployments(&conn) {
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
                    // NOT A BUG: tokio::time::interval fires immediately on creation.
                    // Consuming the first tick here skips that immediate fire so the
                    // first actual refresh happens after 60 minutes, not at startup.
                    interval.tick().await; // consume immediate first tick
                    loop {
                        interval.tick().await; // wait 60 minutes between refreshes
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

    // Warn if infrastructure URLs point at cloud metadata endpoints.
    // QDRANT_URL and REDIS_URL intentionally allow private IPs (these services
    // normally run on internal networks), but cloud metadata IPs (169.254.169.254)
    // are never valid and indicate a dangerous misconfiguration.
    for var_name in &["QDRANT_URL", "REDIS_URL"] {
        if let Ok(url) = std::env::var(var_name) {
            anyllm_proxy::config::warn_if_cloud_metadata_url(var_name, &url);
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

    // Build tool engine state from config, if tool sections were present.
    // Only constructed when at least one of tool_execution / builtin_tools / mcp_servers
    // is present in the config file, to avoid overhead when tools are unused.
    let tool_engine_state: Option<Arc<state::ToolEngineState>> = if let Some(tc) =
        load_result.tool_config.filter(|tc| tc.has_any())
    {
        let simple_config_shell = config::simple::SimpleConfig {
            routing_strategy: None,
            listen_port: None,
            log_bodies: None,
            models: vec![],
            tool_execution: tc.tool_execution,
            builtin_tools: tc.builtin_tools,
            mcp_servers: tc.mcp_servers,
        };
        let (policy, loop_config) = simple_config_shell.build_tool_config();

        let mut registry = tools::ToolRegistry::new();
        // Register built-in tools (gated behind the dangerous-builtin-tools feature).
        anyllm_proxy::tools::builtin::register_all(
            &mut registry,
            simple_config_shell.builtin_tools.as_ref(),
        );

        // Build MCP manager and discover tools from configured servers.
        let mcp_manager = if let Some(ref servers) = simple_config_shell.mcp_servers {
            let manager = Arc::new(tools::McpServerManager::new());
            for server_cfg in servers {
                // SSRF protection: skip servers with private/loopback URLs.
                if let Err(e) = crate::config::validate_base_url(&server_cfg.url) {
                    tracing::error!(
                        server = %server_cfg.name,
                        url = %server_cfg.url,
                        error = %e,
                        "MCP server URL rejected (SSRF protection); skipping"
                    );
                    continue;
                }
                match tools::McpServerManager::discover_tools(&server_cfg.url).await {
                    Ok(discovered) => {
                        tracing::info!(
                            server = %server_cfg.name,
                            url = %server_cfg.url,
                            tools = discovered.len(),
                            "MCP server connected and tools discovered"
                        );
                        if let Err(e) = manager.register_server_blocking(
                            &server_cfg.name,
                            &server_cfg.url,
                            discovered,
                        ) {
                            tracing::error!(
                                server = %server_cfg.name,
                                error = %e,
                                "MCP server registration failed"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            server = %server_cfg.name,
                            url = %server_cfg.url,
                            error = %e,
                            "MCP server unreachable at startup; tools from this server will be unavailable"
                        );
                    }
                }
            }
            // Register all discovered MCP tools into the registry.
            tools::mcp::register_mcp_tools(&manager, &mut registry);
            Some(manager)
        } else {
            None
        };

        tracing::info!(
            registered_tools = registry.list_names().len(),
            mcp_servers = mcp_manager
                .as_ref()
                .map(|m| m.list_servers_blocking().len())
                .unwrap_or(0),
            "tool execution engine initialized"
        );

        Some(Arc::new(state::ToolEngineState {
            registry: Arc::new(registry),
            policy: Arc::new(policy),
            loop_config,
            mcp_manager,
        }))
    } else {
        None
    };

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
    // admin_redirect_port: passed to the proxy router to enable GET / redirect.
    // admin_startup_info: (admin_url, token_str) printed in the startup banner.
    let mut admin_redirect_port: Option<u16> = None;
    let mut admin_startup_info: Option<(String, String)> = None;
    let admin_parts = if enable_admin {
        let admin_port: u16 = match std::env::var("ADMIN_PORT") {
            Ok(val) => val.parse::<u16>().unwrap_or_else(|_| {
                panic!("ADMIN_PORT must be a number in 1-65535, got '{val}'")
            }),
            Err(_) => 3001,
        };
        if admin_port == 0 {
            panic!("ADMIN_PORT cannot be 0");
        }
        if admin_port < 1024 {
            tracing::warn!(
                port = admin_port,
                "ADMIN_PORT is in the privileged range (< 1024); \
                 binding may fail without elevated privileges"
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
                "ADMIN_BIND must be an IP address (e.g. 127.0.0.1 or 0.0.0.0), \
                 not a hostname — got '{}'",
                std::env::var("ADMIN_BIND").unwrap_or_default()
            );
        }

        if admin_port == listen_port {
            panic!("ADMIN_PORT ({admin_port}) must differ from LISTEN_PORT ({listen_port})");
        }

        admin_redirect_port = Some(admin_port);

        let db_path = resolve_db_path(&data_dir);
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
            let conn_guard = db.lock().unwrap_or_else(|e| e.into_inner());
            if let Ok(rows) = admin::db::list_managed_backends(&conn_guard) {
                for row in rows {
                    match anyllm_providers::get_provider(&row.provider_id) {
                        None => {
                            tracing::warn!(
                                provider_id = %row.provider_id,
                                backend_id = %row.id,
                                "managed backend references unknown provider; skipping"
                            );
                        }
                        Some(provider) => {
                            match anyllm_proxy::admin::routes::managed_backends::row_to_backend_config(&row, provider) {
                                None => {
                                    tracing::warn!(
                                        provider_id = %row.provider_id,
                                        backend_id = %row.id,
                                        "managed backend uses unsupported protocol (Custom); skipping"
                                    );
                                }
                                Some(bc) => {
                                    let client = anyllm_proxy::backend::BackendClient::from_backend_config(&bc);
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
        // PROVIDER_AUTO_REFRESH=1|true|yes enables it; default off.
        // PROVIDER_REFRESH_INTERVAL_HOURS controls the interval (default: 168 = 7 days).
        let auto_refresh = matches!(
            std::env::var("PROVIDER_AUTO_REFRESH").as_deref(),
            Ok("1") | Ok("true") | Ok("yes")
        );
        let refresh_interval_hours: u64 = std::env::var("PROVIDER_REFRESH_INTERVAL_HOURS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(168); // default: 7 days

        if auto_refresh {
            let shared_for_refresh = shared.clone();
            let client = PROVIDER_REFRESH_CLIENT.clone();
            tokio::spawn(async move {
                let interval = std::time::Duration::from_secs(refresh_interval_hours * 3600);
                // Delay first run by 30s so startup is not slowed down.
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                loop {
                    for provider in anyllm_providers::all_providers() {
                        if !provider.capabilities.chat_completions {
                            continue;
                        }
                        let api_key =
                            provider.env_vars.iter().find_map(|v| std::env::var(v).ok());
                        if api_key.is_none() {
                            continue; // skip unconfigured providers
                        }

                        let url = format!(
                            "{}/v1/models",
                            provider.default_base_url.trim_end_matches('/')
                        );
                        let mut req = client.get(&url);
                        if let Some(ref key) = api_key {
                            req = req.header("Authorization", format!("Bearer {key}"));
                        }
                        match req.send().await {
                            Err(e) => tracing::warn!(
                                provider = %provider.id,
                                error = %e,
                                "provider auto-refresh failed"
                            ),
                            Ok(resp) if !resp.status().is_success() => tracing::warn!(
                                provider = %provider.id,
                                status = %resp.status(),
                                "provider auto-refresh upstream error"
                            ),
                            Ok(resp) => {
                                match resp.json::<serde_json::Value>().await {
                                    Err(e) => tracing::warn!(
                                        provider = %provider.id,
                                        error = %e,
                                        "provider auto-refresh: invalid JSON response"
                                    ),
                                    Ok(json) => {
                                        let model_ids: Vec<String> = json
                                            .get("data")
                                            .and_then(|d| d.as_array())
                                            .map(|arr| {
                                                arr.iter()
                                                    .filter_map(|m| {
                                                        m.get("id")?.as_str().map(String::from)
                                                    })
                                                    .collect()
                                            })
                                            .unwrap_or_default();
                                        let count = model_ids.len();
                                        let db = shared_for_refresh.db.clone();
                                        let pid = provider.id.to_string();
                                        let _ = tokio::task::spawn_blocking(move || {
                                            let mut conn =
                                                db.lock().unwrap_or_else(|e| e.into_inner());
                                            if let Err(e) = admin::db::upsert_provider_models_cache(
                                                &mut conn,
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
                                            provider = %provider.id,
                                            count = count,
                                            "auto-refreshed provider model cache"
                                        );
                                    }
                                }
                            }
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

        // Admin token: use env var or generate 256-bit random hex written to a file.
        let admin_token = match std::env::var("ADMIN_TOKEN") {
            Ok(t) => {
                if t.len() < 32 {
                    tracing::warn!(
                        len = t.len(),
                        "ADMIN_TOKEN is shorter than 32 characters; \
                         use a longer random value to reduce brute-force risk \
                         (generate one with: openssl rand -hex 32)"
                    );
                }
                t
            }
            Err(_) => {
                let mut buf = [0u8; 32];
                getrandom::fill(&mut buf).expect("getrandom failed");
                let token = hex::encode(buf);
                let token_path = resolve_admin_token_path(&data_dir);
                let token_path = token_path.to_string_lossy().to_string();
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
                    // Log the path and retrieval command, not the token itself.
                    // A new token is generated on every restart; set ADMIN_TOKEN for a fixed value.
                    tracing::info!(
                        path = %token_path,
                        "admin token written to file — \
                         retrieve with: cat {token_path} \
                         | set ADMIN_TOKEN env var to use a fixed token across restarts"
                    );
                }
                token
            }
        };
        let admin_token = Arc::new(zeroize::Zeroizing::new(admin_token));

        // Capture for the startup banner printed after both servers are bound.
        let admin_display_host = if admin_bind == "0.0.0.0" { "localhost" } else { &admin_bind };
        let admin_ui_url = format!("http://{}:{}/admin/", admin_display_host, admin_port);
        admin_startup_info = Some((admin_ui_url, admin_token.as_str().to_owned()));

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
                // Count requests in the last 60 seconds for RPM.
                let now_secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let since = now_secs.saturating_sub(60);
                let rpm = admin::state::with_db(&snapshot_shared.db, move |conn| {
                    admin::db::count_requests_since(conn, since).unwrap_or(0)
                })
                .await
                .unwrap_or(0) as f64;

                let snapshot = admin::state::MetricsSnapshotData {
                    total_requests: aggregate.requests_total,
                    successful_requests: aggregate.requests_success,
                    failed_requests: aggregate.requests_error,
                    requests_per_minute: rpm,
                    // Latency percentiles require a heavier DB query; omit from WS
                    // snapshots (available from GET /admin/api/metrics on demand).
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

        // Spawn background health checker with a snapshot of backend base URLs.
        // base_url lives in BackendConfig (not RuntimeConfig), so we snapshot
        // it here once at startup rather than storing it in SharedState.
        let backend_urls: Vec<(String, String)> = multi_config
            .backends
            .iter()
            .map(|(name, bc)| (name.clone(), bc.base_url.clone()))
            .collect();
        admin::health_check::spawn(shared.clone(), backend_urls);

        // Bind admin listener; spawned after the shutdown channel is created below.
        let admin_app = admin::routes::admin_router(shared.clone(), admin_token);
        let admin_addr = format!("{admin_bind}:{admin_port}");
        let admin_listener = tokio::net::TcpListener::bind(&admin_addr)
            .await
            .unwrap_or_else(|e| panic!("failed to bind admin to {admin_addr}: {e}"));
        tracing::info!("admin listening on {admin_addr}");

        Some((shared, admin_app, admin_listener))
    } else {
        None
    };

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
        let db_path = resolve_db_path(&data_dir);
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
        admin_parts.as_ref().map(|(s, _, _)| s.clone()),
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

    // Print a clear startup banner once both servers are bound, so it appears
    // at the bottom of startup output and is easy to find and copy from.
    if let Some((admin_url, token)) = &admin_startup_info {
        let proxy_display = proxy_addr.replace("0.0.0.0", "localhost");
        let border = "─".repeat(56);
        println!("{border}");
        println!("  Proxy API  http://{proxy_display}");
        println!("  Admin UI   {admin_url}");
        println!("  Token      {token}");
        println!("{border}");
    }

    // Warn if API keys are configured and listener is on a non-loopback address.
    let listen_addr = proxy_listener
        .local_addr()
        .unwrap_or_else(|e| panic!("failed to get local address from listener: {e}"));

    let has_proxy_keys = std::env::var("PROXY_API_KEYS").is_ok();
    let has_virtual_keys = admin_parts
        .as_ref()
        .map(|(shared, _, _)| !shared.virtual_keys.is_empty())
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
    // PROXY_OPEN_RELAY=true accepts any non-empty string as a valid API key;
    // combined with a public bind address this exposes the backend to the internet.
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
        if let Some((_, admin_app, admin_listener)) = admin_parts {
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

/// Parse a `.env`-format file and return `(key, value)` pairs to set.
///
/// Delegates parsing to `anyllm_proxy::env_parser::parse_env_content` (pure, no side effects).
///
/// Hard errors are printed to stderr and result in an empty list; warnings are printed as-is.
/// Already-set environment variables are skipped so the real environment always wins.
/// Compatible with Docker `--env-file` and standard dotenv tooling.
fn parse_env_file(path: &str) -> Vec<(String, String)> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("anyllm_proxy: could not read env file '{path}': {e}");
            return Vec::new();
        }
    };

    let result = anyllm_proxy::env_parser::parse_env_content(&content);

    for err in &result.hard_errors {
        eprintln!("anyllm_proxy: {path}: {err}");
    }
    if !result.hard_errors.is_empty() {
        return Vec::new();
    }

    for warn in &result.warnings {
        let loc = warn.line.map(|l| format!(":{l}")).unwrap_or_default();
        eprintln!("anyllm_proxy: {path}{loc}: {}", warn.message);
    }

    result
        .pairs
        .into_iter()
        .filter(|p| std::env::var(&p.key).is_err())
        .map(|p| (p.key, p.value))
        .collect()
}

/// Load env vars previously imported via the admin UI from the SQLite `env_import` table.
///
/// Opens the database synchronously (rusqlite is sync) before the tokio runtime starts,
/// so `set_var` remains single-threaded safe. Skips keys already present in the environment
/// (real env and .anyllm.env take precedence). Silently succeeds if the DB or table does
/// not yet exist (first run before any import).
fn load_env_from_sqlite(db_path: &str) -> Vec<(String, String)> {
    let conn = match rusqlite::Connection::open(db_path) {
        Ok(c) => c,
        Err(e) => {
            // Benign when the DB file doesn't exist yet (first run).
            // Log for any other open failure (permissions, corruption, etc.).
            if std::path::Path::new(db_path).exists() {
                eprintln!("anyllm_proxy: could not open DB '{db_path}' for env import: {e}");
            }
            return Vec::new();
        }
    };

    // The env_import table is created by init_db() during normal startup.
    // If the proxy has never run with a DB, the table won't exist yet.
    let rows: Vec<(String, String)> =
        match conn.prepare("SELECT key, value FROM env_import ORDER BY key") {
            Ok(mut stmt) => stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .and_then(|mapped| mapped.collect())
                .unwrap_or_default(),
            Err(_) => return Vec::new(), // Table doesn't exist yet
        };

    rows.into_iter()
        .filter(|(key, _)| std::env::var(key).is_err())
        .collect()
}

/// Resolve the data directory where config, DB, and token files live.
/// Priority: ANYLLM_HOME env var > ~/.anyllm/ > CWD (fallback if HOME unresolvable).
/// Creates the directory on first use (mode 0700 on Unix).
fn resolve_data_dir() -> PathBuf {
    let dir = if let Ok(home) = std::env::var("ANYLLM_HOME") {
        PathBuf::from(home)
    } else if let Some(home) = home_dir() {
        home.join(".anyllm")
    } else {
        // No home directory (unusual). Fall back to CWD.
        PathBuf::from(".")
    };

    if !dir.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true).mode(0o700);
            if let Err(e) = builder.create(&dir) {
                eprintln!(
                    "anyllm_proxy: could not create data directory '{}': {e}",
                    dir.display()
                );
            }
        }
        #[cfg(not(unix))]
        {
            if let Err(e) = std::fs::create_dir_all(&dir) {
                eprintln!(
                    "anyllm_proxy: could not create data directory '{}': {e}",
                    dir.display()
                );
            }
        }
    }
    dir
}

/// Cross-platform home directory lookup.
fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

/// Resolve SQLite DB path: ADMIN_DB_PATH env var > data_dir/admin.db.
fn resolve_db_path(data_dir: &std::path::Path) -> String {
    std::env::var("ADMIN_DB_PATH")
        .unwrap_or_else(|_| data_dir.join("admin.db").to_string_lossy().into_owned())
}

/// Resolve admin token file path from `ADMIN_TOKEN_PATH` env var,
/// falling back to `~/.anyllm/.admin_token`.
fn resolve_admin_token_path(data_dir: &std::path::Path) -> PathBuf {
    match std::env::var("ADMIN_TOKEN_PATH") {
        Ok(p) => {
            let path = PathBuf::from(&p);
            // Reject paths containing traversal sequences to prevent writing
            // the admin token to unexpected locations via misconfigured env vars.
            if p.contains("..") {
                panic!("ADMIN_TOKEN_PATH must not contain '..' path traversal: {p}");
            }
            path
        }
        Err(_) => data_dir.join(".admin_token"),
    }
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
    let mut file: std::fs::File = {
        // On non-Unix platforms, file permissions cannot be set to owner-only
        // at creation time. Returning an error forces the caller to panic,
        // requiring the operator to set ADMIN_TOKEN explicitly.
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "auto-generating the admin token file is not supported on non-Unix platforms; \
             set the ADMIN_TOKEN environment variable explicitly",
        ));
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

// ---------------------------------------------------------------------------
// "run" subcommand helpers
// ---------------------------------------------------------------------------

/// Derives the auth token for the spawned tool from PROXY_API_KEYS (first
/// comma-separated entry), falling back to "proxy-user" if unset.
fn derive_auth_token() -> String {
    std::env::var("PROXY_API_KEYS")
        .ok()
        .and_then(|keys| {
            keys.split(',')
                .next()
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty())
        })
        .unwrap_or_else(|| "proxy-user".to_string())
}

/// Returns the env vars to inject into the spawned tool.
///
/// Supported tools and their configurations:
/// - `claude`    — Claude Code: Bearer auth via ANTHROPIC_AUTH_TOKEN, clears ANTHROPIC_API_KEY
/// - `aider`     — Aider: both Anthropic and OpenAI vars (user picks mode via --model)
/// - `codex`     — OpenAI Codex CLI: OpenAI-format vars
/// - `goose`     — Block Goose: GOOSE_PROVIDER__ namespace (requires GOOSE_PROVIDER__TYPE in env)
/// - `opencode`  — OpenCode: inline JSON config via OPENCODE_CONFIG_CONTENT
/// - `gemini`    — Gemini CLI: GEMINI_BASE_URL + GEMINI_API_KEY (sent as x-goog-api-key)
/// - default     — Any Anthropic-compatible CLI (cursor, windsurf, cline, etc.): standard vars
fn tool_env_vars(tool: &str, proxy_url: &str, auth_token: &str) -> Vec<(&'static str, String)> {
    // OpenAI client libs expect the base URL to include /v1; they append /chat/completions.
    let openai_base = format!("{proxy_url}/v1");

    let tool_name = std::path::Path::new(tool)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(tool);

    match tool_name {
        "claude" => vec![
            ("ANTHROPIC_BASE_URL", proxy_url.to_string()),
            ("ANTHROPIC_AUTH_TOKEN", auth_token.to_string()),
            // Must be cleared; otherwise Claude Code falls back to direct Anthropic API.
            ("ANTHROPIC_API_KEY", String::new()),
            ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1".to_string()),
        ],
        "aider" => vec![
            ("ANTHROPIC_BASE_URL", proxy_url.to_string()),
            ("ANTHROPIC_API_KEY", auth_token.to_string()),
            // OpenAI backend: select with `aider --model openai/<model>`
            ("AIDER_OPENAI_API_BASE", openai_base),
            ("OPENAI_API_KEY", auth_token.to_string()),
        ],
        "codex" => vec![
            ("OPENAI_BASE_URL", openai_base),
            ("OPENAI_API_KEY", auth_token.to_string()),
        ],
        "goose" => vec![
            // GOOSE_PROVIDER__TYPE must already be set (e.g., GOOSE_PROVIDER__TYPE=anthropic).
            ("GOOSE_PROVIDER__HOST", proxy_url.to_string()),
            ("GOOSE_PROVIDER__API_KEY", auth_token.to_string()),
        ],
        "opencode" => {
            // OPENCODE_CONFIG_CONTENT is highest priority; avoids needing a config file on disk.
            // Uses @ai-sdk/openai-compatible (bundled with opencode) against the proxy's /v1 endpoint.
            let config_json = serde_json::json!({
                "provider": {
                    "anyllm": {
                        "npm": "@ai-sdk/openai-compatible",
                        "options": {
                            "baseURL": format!("{proxy_url}/v1"),
                            "apiKey": auth_token
                        },
                        "models": {
                            "claude-sonnet-4-6":  {"name": "Claude Sonnet 4.6"},
                            "claude-haiku-4-5":   {"name": "Claude Haiku 4.5"},
                            "gpt-4o":             {"name": "GPT-4o"},
                            "gpt-4o-mini":        {"name": "GPT-4o Mini"}
                        }
                    }
                },
                "model": "anyllm/claude-sonnet-4-6"
            });
            vec![("OPENCODE_CONFIG_CONTENT", config_json.to_string())]
        }
        "gemini" => vec![
            ("GEMINI_BASE_URL", proxy_url.to_string()),
            // Sent as x-goog-api-key; the proxy auth middleware accepts that header.
            ("GEMINI_API_KEY", auth_token.to_string()),
        ],
        _ => vec![
            // Default: standard Anthropic vars (cursor, windsurf, cline, etc.)
            ("ANTHROPIC_BASE_URL", proxy_url.to_string()),
            ("ANTHROPIC_API_KEY", auth_token.to_string()),
        ],
    }
}

/// Polls TCP port `port` on 127.0.0.1 until it accepts a connection or
/// `max_wait_ms` elapses. Returns true if the port became reachable.
fn wait_for_port(port: u16, max_wait_ms: u64) -> bool {
    use std::net::{SocketAddr, TcpStream};
    use std::time::{Duration, Instant};
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().expect("valid addr");
    let deadline = Instant::now() + Duration::from_millis(max_wait_ms);
    loop {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(100)).is_ok() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Implements `anyllm_proxy run <tool> [args...]`:
///
/// 1. Spawns the proxy as a background child process (re-executes self).
/// 2. Waits for the proxy to accept connections on its configured port.
/// 3. Spawns the requested tool with ANTHROPIC_* env vars pointing at the proxy.
/// 4. Waits for the tool to exit, kills the proxy, and returns the tool's exit code.
fn run_subcommand(proxy_args: Vec<String>, tool_argv: Vec<String>) -> i32 {
    let listen_port: u16 = std::env::var("LISTEN_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let auth_token = derive_auth_token();
    let proxy_url = format!("http://localhost:{listen_port}");

    // Re-execute this binary as the proxy server in the background.
    let proxy_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("anyllm_proxy: cannot locate own executable: {e}");
            return 1;
        }
    };

    let mut proxy_child = match std::process::Command::new(&proxy_exe)
        .args(&proxy_args)
        // Signal child to skip env file loading (vars are already inherited).
        .env("_ANYLLM_RUN_CHILD", "1")
        .stderr(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("anyllm_proxy: failed to start proxy: {e}");
            return 1;
        }
    };

    // Wait up to 10 seconds for the proxy to start accepting connections.
    eprintln!("anyllm_proxy: waiting for proxy on port {listen_port}...");
    if !wait_for_port(listen_port, 10_000) {
        eprintln!("anyllm_proxy: proxy did not start within 10 seconds on port {listen_port}");
        let _ = proxy_child.kill();
        let _ = proxy_child.wait();
        return 1;
    }

    let env_vars = tool_env_vars(&tool_argv[0], &proxy_url, &auth_token);

    // Spawn the tool, inheriting all env vars from the parent and overlaying the
    // proxy-specific ones. stdin/stdout/stderr pass through unchanged.
    let exit_code = match std::process::Command::new(&tool_argv[0])
        .args(&tool_argv[1..])
        .envs(env_vars)
        .status()
    {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("anyllm_proxy: failed to run '{}': {e}", tool_argv[0]);
            1
        }
    };

    // Shut down the proxy child.
    let _ = proxy_child.kill();
    let _ = proxy_child.wait();

    exit_code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_env_file_double_quoted_newline_escape() {
        use std::io::Write;
        let dir = std::env::temp_dir();
        let path = dir.join("test_parse_env_escape_n.env");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, r#"KEY="hello\nworld""#).unwrap();
        drop(f);
        let vars = parse_env_file(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
        let val = vars
            .iter()
            .find(|(k, _)| k == "KEY")
            .map(|(_, v)| v.as_str());
        assert_eq!(
            val,
            Some("hello\nworld"),
            "\\n inside double quotes must become a newline"
        );
    }

    #[test]
    fn parse_env_file_double_quoted_tab_escape() {
        use std::io::Write;
        let dir = std::env::temp_dir();
        let path = dir.join("test_parse_env_escape_t.env");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, r#"KEY="col1\tcol2""#).unwrap();
        drop(f);
        let vars = parse_env_file(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
        let val = vars
            .iter()
            .find(|(k, _)| k == "KEY")
            .map(|(_, v)| v.as_str());
        assert_eq!(
            val,
            Some("col1\tcol2"),
            "\\t inside double quotes must become a tab"
        );
    }

    #[test]
    fn parse_env_file_single_quoted_no_escape() {
        use std::io::Write;
        let dir = std::env::temp_dir();
        let path = dir.join("test_parse_env_single.env");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, r#"KEY='hello\nworld'"#).unwrap();
        drop(f);
        let vars = parse_env_file(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
        let val = vars
            .iter()
            .find(|(k, _)| k == "KEY")
            .map(|(_, v)| v.as_str());
        // Single quotes: backslash is literal, no escape processing.
        assert_eq!(
            val,
            Some(r"hello\nworld"),
            "single quotes must not process escapes"
        );
    }
}
