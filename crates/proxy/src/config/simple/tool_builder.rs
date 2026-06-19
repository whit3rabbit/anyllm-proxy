use super::types::SimpleConfig;

impl SimpleConfig {
    /// Convert the YAML tool configuration into the in-memory `ToolExecutionPolicy` + `LoopConfig`
    /// used at request time. Called once at startup after the config file is parsed.
    pub fn build_tool_config(
        &self,
    ) -> (crate::tools::ToolExecutionPolicy, crate::tools::LoopConfig) {
        use crate::tools::policy::{PolicyAction, PolicyRule};

        let mut rules = Vec::new();

        // Builtin tool rules.
        if let Some(ref builtins) = self.builtin_tools {
            for (name, cfg) in builtins {
                if !cfg.enabled {
                    continue;
                }
                let action = match cfg.policy.as_deref() {
                    Some("allow") => PolicyAction::Allow,
                    Some("deny") => PolicyAction::Deny,
                    _ => PolicyAction::PassThrough,
                };
                // Warn loudly when execute_bash is set to Allow: it executes
                // arbitrary OS commands as the proxy process user. Operators
                // should only enable this inside a sandboxed environment.
                if name == "execute_bash" && action == PolicyAction::Allow {
                    tracing::warn!(
                        "execute_bash policy is Allow: the LLM can execute arbitrary OS \
                         commands as the proxy process user. Only enable this inside an \
                         isolated sandbox (seccomp, read-only rootfs, network isolation)."
                    );
                }
                rules.push(PolicyRule {
                    tool_name: name.clone(),
                    action,
                    timeout: cfg.timeout_secs.map(std::time::Duration::from_secs),
                    max_concurrency: None,
                });
            }
        }

        // MCP server rules: glob rule per server for prefixed tool names.
        if let Some(ref servers) = self.mcp_servers {
            for server in servers {
                let action = match server.policy.as_deref() {
                    Some("allow") => PolicyAction::Allow,
                    Some("deny") => PolicyAction::Deny,
                    _ => PolicyAction::PassThrough,
                };
                rules.push(PolicyRule {
                    tool_name: format!("mcp_{}_*", server.name),
                    action,
                    timeout: None,
                    max_concurrency: None,
                });
            }
        }

        let policy = crate::tools::ToolExecutionPolicy {
            default_action: PolicyAction::PassThrough,
            rules,
        };

        let loop_config = if let Some(ref te) = self.tool_execution {
            crate::tools::LoopConfig {
                max_iterations: te.max_iterations.unwrap_or(1),
                tool_timeout: std::time::Duration::from_secs(te.tool_timeout_secs.unwrap_or(30)),
                total_timeout: std::time::Duration::from_secs(te.total_timeout_secs.unwrap_or(300)),
                max_tool_calls_per_turn: te.max_tool_calls_per_turn.unwrap_or(16),
            }
        } else {
            crate::tools::LoopConfig::default()
        };

        (policy, loop_config)
    }
}
