use anyllm_client::http::{build_http_client, HttpClientConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// An MCP tool definition discovered from a server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// An MCP server connection with its discovered tools.
#[derive(Debug, Clone, Serialize)]
pub struct McpServer {
    pub name: String,
    pub url: String,
    pub tools: Vec<McpToolDef>,
}

/// Validate that an MCP server name is safe to use in tool key construction.
/// Allows alphanumerics and hyphens only. Underscores are forbidden because
/// the tool name scheme uses `mcp_{server}_{tool}`; an underscore in the server
/// name makes `parse_mcp_tool_name` ambiguous and can cause tool misrouting.
pub fn is_valid_mcp_server_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '-')
}

/// Build a prefixed tool name: mcp_{server}_{tool}.
pub fn mcp_tool_name(server_name: &str, tool_name: &str) -> String {
    format!("mcp_{}_{}", server_name, tool_name)
}

/// Extract (server_name, original_tool_name) from a prefixed MCP tool name.
pub fn parse_mcp_tool_name(prefixed: &str) -> Option<(&str, &str)> {
    let rest = prefixed.strip_prefix("mcp_")?;
    let underscore_pos = rest.find('_')?;
    let server = &rest[..underscore_pos];
    let tool = &rest[underscore_pos + 1..];
    if tool.is_empty() {
        return None;
    }
    Some((server, tool))
}

/// Manages MCP server connections and tool-to-server routing.
pub struct McpServerManager {
    servers: RwLock<HashMap<String, McpServer>>,
    tool_to_server: RwLock<HashMap<String, String>>,
    /// Shared HTTP client; reqwest::Client is cheaply cloneable (Arc internally).
    client: reqwest::Client,
}

impl McpServerManager {
    pub fn new() -> Self {
        // Use SSRF-safe DNS resolver to prevent DNS rebinding attacks where a
        // domain passes the registration-time URL check but later resolves to a
        // private/metadata IP (e.g., 169.254.169.254) after DNS TTL expiry.
        let client = build_http_client(&HttpClientConfig {
            ssrf_protection: true,
            ..Default::default()
        });
        Self {
            servers: RwLock::new(HashMap::new()),
            tool_to_server: RwLock::new(HashMap::new()),
            client,
        }
    }

    pub fn register_server_blocking(
        &self,
        name: &str,
        url: &str,
        tools: Vec<McpToolDef>,
    ) -> Result<(), String> {
        if !is_valid_mcp_server_name(name) {
            return Err(format!(
                "invalid MCP server name '{}': only alphanumerics and hyphens are allowed",
                name
            ));
        }
        self.remove_server_blocking(name);
        let mut tool_map = self.tool_to_server.write().unwrap();
        for tool in &tools {
            tool_map.insert(mcp_tool_name(name, &tool.name), name.to_string());
        }
        let server = McpServer {
            name: name.to_string(),
            url: url.to_string(),
            tools,
        };
        self.servers
            .write()
            .unwrap()
            .insert(name.to_string(), server);
        Ok(())
    }

    pub fn remove_server_blocking(&self, name: &str) {
        if let Some(server) = self.servers.write().unwrap().remove(name) {
            let mut tool_map = self.tool_to_server.write().unwrap();
            for tool in &server.tools {
                tool_map.remove(&mcp_tool_name(name, &tool.name));
            }
        }
    }

    pub fn list_servers_blocking(&self) -> Vec<McpServer> {
        self.servers.read().unwrap().values().cloned().collect()
    }

    pub fn find_server_for_tool_blocking(&self, prefixed_name: &str) -> Option<String> {
        self.tool_to_server
            .read()
            .unwrap()
            .get(prefixed_name)
            .cloned()
    }

    pub fn as_anthropic_tools_blocking(&self) -> Vec<anyllm_translate::anthropic::Tool> {
        let servers = self.servers.read().unwrap();
        let mut result = Vec::new();
        for server in servers.values() {
            for tool in &server.tools {
                result.push(anyllm_translate::anthropic::Tool {
                    name: mcp_tool_name(&server.name, &tool.name),
                    description: Some(tool.description.clone()),
                    input_schema: tool.input_schema.clone(),
                });
            }
        }
        result
    }

    /// Call an MCP tool via JSON-RPC POST. Returns the result value or error.
    pub async fn call_tool(&self, prefixed_name: &str, input: Value) -> Result<Value, String> {
        let (server_name, original_name) = parse_mcp_tool_name(prefixed_name)
            .ok_or_else(|| format!("invalid MCP tool name: {}", prefixed_name))?;

        let server_url = {
            let servers = self.servers.read().unwrap();
            servers
                .get(server_name)
                .ok_or_else(|| format!("MCP server '{}' not found", server_name))?
                .url
                .clone()
        };

        let client = &self.client;
        let rpc_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": original_name, "arguments": input }
        });

        let response = client
            .post(&server_url)
            .json(&rpc_request)
            .send()
            .await
            .map_err(|e| format!("MCP request to '{}' failed: {}", server_name, e))?;

        if !response.status().is_success() {
            return Err(format!(
                "MCP server '{}' returned status {}",
                server_name,
                response.status()
            ));
        }

        let body: Value = response
            .json()
            .await
            .map_err(|e| format!("MCP response parse error: {}", e))?;

        if let Some(error) = body.get("error") {
            let msg = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown MCP error");
            return Err(format!("MCP tool error: {}", msg));
        }

        body.get("result")
            .cloned()
            .ok_or_else(|| "MCP response missing 'result' field".to_string())
    }

    /// Discover tools from an MCP server using the manager's shared HTTP client.
    pub async fn discover_tools_with_client(&self, url: &str) -> Result<Vec<McpToolDef>, String> {
        discover_tools_impl(&self.client, url).await
    }

    /// Discover tools from an MCP server by calling tools/list.
    ///
    /// Prefer `discover_tools_with_client` when called on a manager instance.
    /// This static version creates a one-shot client; use it only for startup
    /// paths where no manager instance exists yet.
    pub async fn discover_tools(url: &str) -> Result<Vec<McpToolDef>, String> {
        let client = build_http_client(&HttpClientConfig {
            ssrf_protection: true,
            ..Default::default()
        });
        discover_tools_impl(&client, url).await
    }
}

async fn discover_tools_impl(
    client: &reqwest::Client,
    url: &str,
) -> Result<Vec<McpToolDef>, String> {
    let rpc_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list",
        "params": {}
    });

    let response = client
        .post(url)
        .json(&rpc_request)
        .send()
        .await
        .map_err(|e| format!("MCP discovery failed for '{}': {}", url, e))?;

    if !response.status().is_success() {
        return Err(format!(
            "MCP discovery returned status {} for '{}'",
            response.status(),
            url
        ));
    }

    let body: Value = response
        .json()
        .await
        .map_err(|e| format!("MCP discovery parse error: {}", e))?;

    if let Some(error) = body.get("error") {
        let msg = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return Err(format!("MCP discovery error: {}", msg));
    }

    let tools_value = body
        .get("result")
        .and_then(|r| r.get("tools"))
        .ok_or_else(|| "MCP response missing result.tools".to_string())?;

    serde_json::from_value(tools_value.clone()).map_err(|e| format!("MCP tools parse error: {}", e))
}

impl Default for McpServerManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Adapter that wraps an MCP tool as a `Tool` trait implementor.
/// Delegates execution to `McpServerManager::call_tool()`.
pub struct McpToolAdapter {
    pub prefixed_name: String,
    pub description: String,
    pub input_schema: Value,
    pub manager: Arc<McpServerManager>,
}

impl crate::tools::registry::Tool for McpToolAdapter {
    fn name(&self) -> &str {
        &self.prefixed_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }

    fn execute<'a>(
        &'a self,
        input: Value,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, String>> + Send + 'a>>
    {
        Box::pin(async move { self.manager.call_tool(&self.prefixed_name, input).await })
    }
}

/// Register all MCP tools from the manager into a ToolRegistry.
pub fn register_mcp_tools(
    manager: &Arc<McpServerManager>,
    registry: &mut crate::tools::ToolRegistry,
) {
    let servers = manager.list_servers_blocking();
    for server in &servers {
        for tool in &server.tools {
            let prefixed = mcp_tool_name(&server.name, &tool.name);
            let adapter = McpToolAdapter {
                prefixed_name: prefixed,
                description: tool.description.clone(),
                input_schema: tool.input_schema.clone(),
                manager: manager.clone(),
            };
            registry.register(Box::new(adapter));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manager_starts_empty() {
        let mgr = McpServerManager::new();
        assert!(mgr.list_servers_blocking().is_empty());
        assert!(mgr.find_server_for_tool_blocking("anything").is_none());
    }

    #[test]
    fn register_server_maps_tools() {
        let mgr = McpServerManager::new();
        let tools = vec![
            McpToolDef {
                name: "search_repos".into(),
                description: "Search".into(),
                input_schema: serde_json::json!({"type": "object"}),
            },
            McpToolDef {
                name: "create_issue".into(),
                description: "Create".into(),
                input_schema: serde_json::json!({"type": "object"}),
            },
        ];
        mgr.register_server_blocking("github", "https://example.com/sse", tools)
            .unwrap();

        let servers = mgr.list_servers_blocking();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].tools.len(), 2);
        assert_eq!(
            mgr.find_server_for_tool_blocking("mcp_github_search_repos"),
            Some("github".to_string())
        );
        assert_eq!(
            mgr.find_server_for_tool_blocking("mcp_github_create_issue"),
            Some("github".to_string())
        );
        assert!(mgr
            .find_server_for_tool_blocking("mcp_slack_send")
            .is_none());
    }

    #[test]
    fn remove_server_cleans_up() {
        let mgr = McpServerManager::new();
        mgr.register_server_blocking(
            "github",
            "https://example.com/sse",
            vec![McpToolDef {
                name: "search".into(),
                description: "s".into(),
                input_schema: serde_json::json!({"type": "object"}),
            }],
        )
        .unwrap();
        assert!(mgr
            .find_server_for_tool_blocking("mcp_github_search")
            .is_some());
        mgr.remove_server_blocking("github");
        assert!(mgr.list_servers_blocking().is_empty());
        assert!(mgr
            .find_server_for_tool_blocking("mcp_github_search")
            .is_none());
    }

    #[test]
    fn mcp_tool_name_prefixing() {
        assert_eq!(mcp_tool_name("github", "search"), "mcp_github_search");
        assert_eq!(
            mcp_tool_name("my-server", "do_thing"),
            "mcp_my-server_do_thing"
        );
    }

    #[test]
    fn mcp_tool_adapter_implements_tool_trait() {
        let mgr = Arc::new(McpServerManager::new());
        let adapter = McpToolAdapter {
            prefixed_name: "mcp_github_search".to_string(),
            description: "Search repos".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
            manager: mgr,
        };
        use crate::tools::registry::Tool;
        assert_eq!(adapter.name(), "mcp_github_search");
        assert_eq!(adapter.description(), "Search repos");
    }

    #[test]
    fn register_mcp_tools_into_registry() {
        let mgr = Arc::new(McpServerManager::new());
        mgr.register_server_blocking(
            "github",
            "https://example.com/sse",
            vec![McpToolDef {
                name: "search".to_string(),
                description: "Search".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
            }],
        )
        .unwrap();
        let mut registry = crate::tools::ToolRegistry::new();
        register_mcp_tools(&mgr, &mut registry);
        assert!(registry.contains("mcp_github_search"));
    }

    #[test]
    fn as_anthropic_tools_returns_prefixed() {
        let mgr = McpServerManager::new();
        mgr.register_server_blocking(
            "github",
            "https://example.com/sse",
            vec![McpToolDef {
                name: "search".into(),
                description: "Search".into(),
                input_schema: serde_json::json!({"type": "object", "properties": {}}),
            }],
        )
        .unwrap();
        let tools = mgr.as_anthropic_tools_blocking();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "mcp_github_search");
    }

    #[test]
    fn valid_server_names_accepted() {
        assert!(is_valid_mcp_server_name("github"));
        assert!(is_valid_mcp_server_name("my-server"));
        assert!(is_valid_mcp_server_name("Server1"));
    }

    #[test]
    fn invalid_server_names_rejected() {
        assert!(!is_valid_mcp_server_name(""));
        assert!(!is_valid_mcp_server_name("my_server")); // underscore
        assert!(!is_valid_mcp_server_name("bad name")); // space
        assert!(!is_valid_mcp_server_name("a/b")); // slash
    }

    #[test]
    fn register_server_blocking_rejects_underscored_names() {
        let mgr = McpServerManager::new();
        let err = mgr
            .register_server_blocking("evil_server", "https://example.com", vec![])
            .unwrap_err();
        assert!(err.contains("invalid MCP server name"));
        assert!(mgr.list_servers_blocking().is_empty());
    }
}
