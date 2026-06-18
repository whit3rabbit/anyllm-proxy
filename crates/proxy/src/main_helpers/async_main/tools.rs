use anyllm_proxy::config;
use anyllm_proxy::server::state;
use anyllm_proxy::tools;
use std::sync::Arc;

pub(crate) async fn init_tool_engine(
    tool_config: Option<config::simple::ToolStartupConfig>,
) -> Option<Arc<state::ToolEngineState>> {
    let tc = tool_config.filter(|tc| tc.has_any())?;
    let simple_config_shell = config::simple::SimpleConfig {
        routing_strategy: None,
        listen_port: None,
        log_bodies: None,
        redact_secrets: None,
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
        mcp_manager,
    }))
}
