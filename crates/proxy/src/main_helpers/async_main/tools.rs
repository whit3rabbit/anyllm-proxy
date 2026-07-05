use anyllm_proxy::config;
use anyllm_proxy::server::state;
use anyllm_proxy::tools;
use std::sync::Arc;

pub(crate) async fn init_tool_engine(
    tool_config: Option<config::simple::ToolStartupConfig>,
) -> Option<Arc<state::ToolEngineState>> {
    // Whether any tool_execution/builtin_tools/mcp_servers section was present
    // in YAML. When it wasn't, FORGE_TOOL_CALL_POLICY alone must still be able
    // to build a minimal (empty registry, default policy/loop_config) engine
    // below -- so the env-var check must not be gated behind an early return
    // here.
    let had_yaml_tool_config = tool_config.as_ref().is_some_and(|tc| tc.has_any());
    let tc = tool_config
        .filter(|tc| tc.has_any())
        .unwrap_or(config::simple::ToolStartupConfig {
            tool_execution: None,
            builtin_tools: None,
            mcp_servers: None,
        });
    let simple_config_shell = config::simple::SimpleConfig {
        routing_strategy: None,
        listen_port: None,
        log_bodies: None,
        redact_secrets: None,
        anthropic_thinking_repair: None,
        pxpipe_compress: None,
        models: vec![],
        tool_execution: tc.tool_execution,
        builtin_tools: tc.builtin_tools,
        mcp_servers: tc.mcp_servers,
    };
    let (policy, loop_config) = simple_config_shell.build_tool_config();
    let mut guardrails = simple_config_shell.build_tool_guardrail_config();
    let guardrails_set_in_yaml = simple_config_shell
        .tool_execution
        .as_ref()
        .and_then(|tool_execution| tool_execution.guardrails.as_ref())
        .is_some();
    // `build_tool_guardrail_config` guarantees `guardrails_set_in_yaml ==
    // false` implies `guardrails.mode == Disabled` for every input, so
    // checking the mode here too would be dead weight -- `!guardrails_set_in_yaml`
    // alone already means "there's no YAML mode to defer to".
    if !guardrails_set_in_yaml {
        if let Some(mode) = std::env::var("FORGE_TOOL_CALL_POLICY")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            match mode.parse::<tools::ToolGuardrailMode>() {
                Ok(parsed) => {
                    // Preserve a YAML-set max_write_payload_bytes across the
                    // env-driven preset rebuild instead of resetting it back
                    // to ToolGuardrailConfig::standard()'s default.
                    let max_write_payload_bytes = guardrails.max_write_payload_bytes;
                    guardrails = tools::ToolGuardrailConfig::from_mode(parsed);
                    guardrails.max_write_payload_bytes = max_write_payload_bytes;
                }
                Err(err) => {
                    tracing::warn!("invalid FORGE_TOOL_CALL_POLICY value: {err}; ignoring");
                }
            }
        }
    }

    // No YAML tool sections were configured, and FORGE_TOOL_CALL_POLICY was
    // unset/empty/invalid/disabled: preserve today's behavior of not standing
    // up a tool engine at all.
    if !had_yaml_tool_config && guardrails.mode == tools::ToolGuardrailMode::Disabled {
        return None;
    }

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
            if let Err(e) = anyllm_proxy::config::validate_base_url(&server_cfg.url) {
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
        guardrails,
        mcp_manager,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Env vars are process-global. `anyllm_proxy::config::ENV_TEST_LOCK` is
    // `pub(crate)` to the lib crate and not reachable from here (this file
    // is compiled as part of the `anyllm-proxy` bin crate, a separate test
    // binary/process from the lib's own tests), so this module needs its
    // own lock to serialize FORGE_TOOL_CALL_POLICY mutation across the
    // tests in this file. `init_tool_engine` is async and is awaited while
    // the lock is held, so this must be a `tokio::sync::Mutex`, not
    // `std::sync::Mutex` (clippy::await_holding_lock).
    static FORGE_POLICY_ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn set_forge_policy_env(value: Option<&str>) {
        unsafe {
            match value {
                Some(v) => std::env::set_var("FORGE_TOOL_CALL_POLICY", v),
                None => std::env::remove_var("FORGE_TOOL_CALL_POLICY"),
            }
        }
    }

    #[tokio::test]
    async fn forge_tool_call_policy_env_only() {
        // (a) tool_config None + FORGE_TOOL_CALL_POLICY=standard => Some
        // engine with guardrails.mode != Disabled, even though no YAML
        // tool_execution/builtin_tools/mcp_servers section was present.
        let _lock = FORGE_POLICY_ENV_LOCK.lock().await;
        let previous = std::env::var("FORGE_TOOL_CALL_POLICY").ok();
        set_forge_policy_env(Some("standard"));

        let engine = init_tool_engine(None).await;

        set_forge_policy_env(previous.as_deref());
        let engine = engine.expect("env-only FORGE_TOOL_CALL_POLICY=standard must build an engine");
        assert_ne!(engine.guardrails.mode, tools::ToolGuardrailMode::Disabled);
    }

    #[tokio::test]
    async fn no_env_and_no_yaml_tool_sections_yields_none() {
        // (b) env unset + no tool sections => None
        let _lock = FORGE_POLICY_ENV_LOCK.lock().await;
        let previous = std::env::var("FORGE_TOOL_CALL_POLICY").ok();
        set_forge_policy_env(None);

        let engine = init_tool_engine(None).await;

        set_forge_policy_env(previous.as_deref());
        assert!(engine.is_none());
    }

    #[tokio::test]
    async fn invalid_forge_tool_call_policy_does_not_panic_and_yields_none() {
        // (c) invalid env value => None (no YAML sections present, and the
        // invalid value is ignored rather than parsed), and must not panic.
        let _lock = FORGE_POLICY_ENV_LOCK.lock().await;
        let previous = std::env::var("FORGE_TOOL_CALL_POLICY").ok();
        set_forge_policy_env(Some("not-a-real-mode"));

        let engine = init_tool_engine(None).await;

        set_forge_policy_env(previous.as_deref());
        assert!(engine.is_none());
    }
}
