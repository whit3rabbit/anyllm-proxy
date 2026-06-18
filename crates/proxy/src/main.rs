mod main_helpers;

use anyllm_proxy::config;

/// Entry point: set env vars while still single-threaded (before tokio runtime),
/// then hand off to the async main. This avoids UB from calling set_var after
/// the multi-thread tokio runtime has spawned worker threads.
fn main() {
    let args: Vec<String> = std::env::args().collect();

    // When spawned as a proxy child by the "run" subcommand, env vars are already
    // inherited from the parent process; skip env file loading to avoid duplicate messages.
    let is_run_child = std::env::var("_ANYLLM_RUN_CHILD").is_ok();

    // Resolve the data directory early so all path defaults can use it.
    let data_dir = main_helpers::bootstrap::resolve_data_dir();
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
            .map(main_helpers::bootstrap::parse_env_file)
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
        let db_path = main_helpers::bootstrap::resolve_db_path(&data_dir);
        let db_vars = main_helpers::bootstrap::load_env_from_sqlite(&db_path);
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

    if args.iter().any(|arg| arg == "--redact-secrets") {
        unsafe { std::env::set_var("REDACT_SECRETS", "true") };
        eprintln!("anyllm_proxy: REDACT_SECRETS enabled by --redact-secrets");
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
        std::process::exit(main_helpers::run_cmd::run_subcommand(proxy_args, tool_argv));
    }

    // Detect "providers" subcommand: anyllm-proxy providers list [--json]
    //                                anyllm-proxy providers refresh <id>
    //                                anyllm-proxy providers refresh --all
    if let Some(pos) = args.iter().position(|a| a == "providers") {
        let subcmd_args: Vec<String> = args[pos + 1..].to_vec();
        std::process::exit(main_helpers::providers_cmd::providers_subcommand(
            subcmd_args,
            &data_dir,
        ));
    }

    // Now start the tokio runtime and enter the async main.
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime")
        .block_on(main_helpers::async_main::async_main(args, data_dir));
}
