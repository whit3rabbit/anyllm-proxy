mod main_helpers;

use anyllm_proxy::config;

/// Entry point: set env vars while still single-threaded (before tokio runtime),
/// then hand off to the async main. This avoids UB from calling set_var after
/// the multi-thread tokio runtime has spawned worker threads.
fn main() {
    let mut args: Vec<String> = std::env::args().collect();

    // Parse and strip a leading --port/-p flag (before any `run`/`providers`
    // subcommand, so flags meant for the launched tool are left intact) and apply
    // it as LISTEN_PORT. Pure scanning lives in `strip_port_flag` so it stays
    // unit-testable without env mutation, bin tests can't use ENV_TEST_LOCK
    // (see crates/proxy/CLAUDE.md). The env write stays here, single-threaded
    // before the tokio runtime starts.
    let subcmd_idx = args.iter().position(|a| a == "run" || a == "providers");
    let limit = subcmd_idx.unwrap_or(args.len());
    let (clean_args, port_flag) = strip_port_flag(&args, limit);
    match port_flag {
        PortFlag::Port(port) => {
            // SAFETY: single-threaded here (no tokio runtime yet).
            unsafe { std::env::set_var("LISTEN_PORT", port.to_string()) };
        }
        PortFlag::MissingValue => {
            eprintln!("error: --port / -p requires a port number argument");
            std::process::exit(1);
        }
        PortFlag::InvalidValue(value) => {
            eprintln!("error: invalid port number '{value}'");
            std::process::exit(1);
        }
        PortFlag::None => {}
    }
    args = clean_args;

    // When spawned as a proxy child by the "run" subcommand, env vars are already
    // inherited from the parent process; skip env file loading to avoid duplicate messages.
    let is_run_child = std::env::var("_ANYLLM_RUN_CHILD").is_ok();

    // Admin mode is on when --webui/--admin is passed, or the binary is run with no
    // args at all (zero-arg default), and not force-disabled. Shares the same gate as
    // init_admin. Used to keep startup output quiet in that mode.
    let admin_mode = main_helpers::bootstrap::admin_enabled(&args);

    // Resolve the data directory early so all path defaults can use it.
    let data_dir = main_helpers::bootstrap::resolve_data_dir();
    if !is_run_child {
        eprintln!("anyllm-proxy: data directory: {}", data_dir.display());
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
                "anyllm-proxy: loaded {} variable(s) from env file",
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
                "anyllm-proxy: applied {} variable(s) from admin DB env import",
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
            eprintln!("anyllm-proxy: auto-detected config: {path_str}");
        }
    }

    if args.iter().any(|arg| arg == "--redact-secrets") {
        unsafe { std::env::set_var("REDACT_SECRETS", "true") };
        eprintln!("anyllm-proxy: REDACT_SECRETS enabled by --redact-secrets");
    }

    // Extract litellm master_key before the runtime starts (still single-threaded).
    if std::env::var("PROXY_API_KEYS").is_err() {
        if let Ok(ref config_path) = std::env::var("PROXY_CONFIG") {
            if let Some(mk) = config::extract_litellm_master_key(config_path) {
                // SAFETY: genuinely single-threaded here (no tokio runtime yet).
                unsafe { std::env::set_var("PROXY_API_KEYS", &mk) };
                eprintln!("anyllm-proxy: applied general_settings.master_key as PROXY_API_KEYS");
            }
        }
    }

    // Warn when no backend is configured so users aren't left guessing why
    // requests fail. Skip when spawned as a child of the "run" subcommand.
    if !is_run_child && !anyllm_proxy::admin::routes::status::is_backend_configured() {
        // In admin mode the UI shows a getting-started guide, so keep the CLI
        // quiet — one line instead of the full backend cheat-sheet.
        if admin_mode {
            eprintln!(
                "\nanyllm-proxy: no backend configured yet — open the admin UI to set one up.\n"
            );
        } else {
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
    }

    // Detect "run" subcommand: anyllm-proxy [proxy_opts...] run <command> [args...]
    // Starts the proxy in the background and launches <command> with the proxy's
    // ANTHROPIC_* env vars pre-configured, then exits when <command> exits.
    if let Some(run_idx) = args.iter().position(|a| a == "run") {
        let tool_argv: Vec<String> = args[run_idx + 1..].to_vec();
        if tool_argv.is_empty() {
            eprintln!("usage: anyllm-proxy [--env-file FILE] run <command> [args...]");
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

/// Outcome of scanning args for a `--port`/`-p` flag.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PortFlag {
    /// No flag found.
    None,
    /// Valid port supplied.
    Port(u16),
    /// Flag present but no value followed it before the subcommand boundary.
    MissingValue,
    /// Flag present but the value was not a valid u16.
    InvalidValue(String),
}

/// Pure: scan `args[..limit]` for `--port`/`-p <PORT>` pairs and return the
/// outcome plus `args` with every matched pair removed. Pairs at or past
/// `limit` (i.e. after a `run`/`providers` subcommand) are left untouched so
/// flags meant for the launched tool survive. Stops consuming after the first
/// malformed value, matching the old inline loop's fail-fast exit. No env I/O,
/// no process exit, so it is unit-testable from the bin crate.
fn strip_port_flag(args: &[String], limit: usize) -> (Vec<String>, PortFlag) {
    let mut clean = Vec::with_capacity(args.len());
    let mut port_flag = PortFlag::None;
    let mut i = 0;
    while i < args.len() {
        let seen_error = matches!(
            port_flag,
            PortFlag::InvalidValue(_) | PortFlag::MissingValue
        );
        if !seen_error && i < limit && (args[i] == "--port" || args[i] == "-p") {
            if i + 1 < limit {
                match args[i + 1].parse::<u16>() {
                    Ok(port) => port_flag = PortFlag::Port(port),
                    Err(_) => port_flag = PortFlag::InvalidValue(args[i + 1].clone()),
                }
                i += 2;
            } else {
                port_flag = PortFlag::MissingValue;
                i += 1;
            }
        } else {
            clean.push(args[i].clone());
            i += 1;
        }
    }
    (clean, port_flag)
}

#[cfg(test)]
mod tests {
    use super::{strip_port_flag, PortFlag};

    // Args always start with the binary name (argv[0]); mirror that here.
    fn args(parts: &[&str]) -> Vec<String> {
        let mut v = vec!["anyllm-proxy".to_string()];
        v.extend(parts.iter().map(|s| s.to_string()));
        v
    }

    #[test]
    fn long_flag_parsed_and_stripped() {
        let (clean, flag) = strip_port_flag(&args(&["--port", "3001"]), 3);
        assert_eq!(clean, args(&[]));
        assert_eq!(flag, PortFlag::Port(3001));
    }

    #[test]
    fn short_flag_parsed_and_stripped() {
        let (_clean, flag) = strip_port_flag(&args(&["-p", "8080"]), 3);
        assert_eq!(flag, PortFlag::Port(8080));
    }

    #[test]
    fn no_flag_passes_through_untouched() {
        let (clean, flag) = strip_port_flag(&args(&["--webui"]), 2);
        assert_eq!(clean, args(&["--webui"]));
        assert!(matches!(flag, PortFlag::None));
    }

    #[test]
    fn invalid_value_is_reported() {
        let (_clean, flag) = strip_port_flag(&args(&["--port", "not-a-port"]), 3);
        assert_eq!(flag, PortFlag::InvalidValue("not-a-port".to_string()));
    }

    #[test]
    fn missing_value_at_tail_is_reported() {
        let (clean, flag) = strip_port_flag(&args(&["--port"]), 2);
        // No value followed; the orphan flag is consumed (dropped from clean),
        // matching the original inline loop, and main() exits on MissingValue.
        assert_eq!(clean, args(&[]));
        assert!(matches!(flag, PortFlag::MissingValue));
    }

    #[test]
    fn flag_after_run_subcommand_is_not_consumed() {
        // `run echo --port 3001`: the subcommand owns everything after `run`.
        let input = args(&["run", "echo", "--port", "3001"]);
        let limit = 1; // "run" at index 1 bounds the proxy-args region.
        let (clean, flag) = strip_port_flag(&input, limit);
        assert_eq!(clean, input);
        assert!(matches!(flag, PortFlag::None));
    }

    #[test]
    fn flag_before_run_subcommand_is_consumed_and_run_args_preserved() {
        let input = args(&["--port", "3001", "run", "echo"]);
        let limit = 3; // "run" at index 3.
        let (clean, flag) = strip_port_flag(&input, limit);
        assert_eq!(clean, args(&["run", "echo"]));
        assert_eq!(flag, PortFlag::Port(3001));
    }

    #[test]
    fn last_port_wins_when_flag_repeated() {
        let (_clean, flag) = strip_port_flag(&args(&["--port", "3000", "--port", "4000"]), 5);
        assert_eq!(flag, PortFlag::Port(4000));
    }

    #[test]
    fn first_malformed_value_short_circuits() {
        // `--port abc --port 4000`: stops at `abc`, leaves the rest intact.
        let (clean, flag) = strip_port_flag(&args(&["--port", "abc", "--port", "4000"]), 5);
        assert_eq!(flag, PortFlag::InvalidValue("abc".to_string()));
        assert!(clean.contains(&"--port".to_string()));
    }
}
