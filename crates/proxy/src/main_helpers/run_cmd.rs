/// Derives the auth token for the spawned tool from PROXY_API_KEYS (first
/// comma-separated entry), falling back to "proxy-user" if unset.
pub fn derive_auth_token() -> String {
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
pub fn tool_env_vars(tool: &str, proxy_url: &str, auth_token: &str) -> Vec<(&'static str, String)> {
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
pub fn wait_for_port(port: u16, max_wait_ms: u64) -> bool {
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
pub fn run_subcommand(proxy_args: Vec<String>, tool_argv: Vec<String>) -> i32 {
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
