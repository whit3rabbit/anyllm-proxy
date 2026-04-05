use crate::admin::state::SharedState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};

/// GET /admin/api/mcp-servers - List all registered MCP servers and their tools.
pub(super) async fn list_mcp_servers(
    State(shared): State<SharedState>,
) -> axum::response::Response {
    let Some(ref mgr) = shared.mcp_manager else {
        return (StatusCode::OK, Json(serde_json::json!({"servers": []}))).into_response();
    };
    let servers = mgr.list_servers_blocking();
    (
        StatusCode::OK,
        Json(serde_json::json!({"servers": servers})),
    )
        .into_response()
}

/// POST /admin/api/mcp-servers - Register an MCP server. Body: { name, url }.
/// Performs tool discovery via JSON-RPC tools/list before registering.
pub(super) async fn add_mcp_server(
    State(shared): State<SharedState>,
    Json(body): Json<serde_json::Value>,
) -> axum::response::Response {
    let Some(ref mgr) = shared.mcp_manager else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "MCP support not enabled"})),
        )
            .into_response();
    };

    let name = match body.get("name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "missing 'name' field"})),
            )
                .into_response()
        }
    };
    let url = match body.get("url").and_then(|v| v.as_str()) {
        Some(u) => u.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "missing 'url' field"})),
            )
                .into_response()
        }
    };

    // SSRF protection: reject private/loopback IPs and reserved hostnames.
    if let Err(e) = crate::config::validate_base_url(&url) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("invalid MCP server URL: {e}")})),
        )
            .into_response();
    }

    match crate::tools::mcp::McpServerManager::discover_tools(&url).await {
        Ok(tools) => {
            let tool_count = tools.len();
            if let Err(e) = mgr.register_server_blocking(&name, &url, tools) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": e})),
                )
                    .into_response();
            }
            tracing::info!(server = %name, tools = tool_count, "MCP server registered");
            (
                StatusCode::CREATED,
                Json(serde_json::json!({
                    "name": name,
                    "url": url,
                    "tools_discovered": tool_count,
                })),
            )
                .into_response()
        }
        Err(e) => {
            tracing::warn!(server = %name, error = %e, "MCP tool discovery failed");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": e})),
            )
                .into_response()
        }
    }
}

/// DELETE /admin/api/mcp-servers/:name - Remove a registered MCP server.
pub(super) async fn remove_mcp_server(
    State(shared): State<SharedState>,
    Path(name): Path<String>,
) -> axum::response::Response {
    let Some(ref mgr) = shared.mcp_manager else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "MCP support not enabled"})),
        )
            .into_response();
    };
    mgr.remove_server_blocking(&name);
    tracing::info!(server = %name, "MCP server removed");
    (StatusCode::OK, Json(serde_json::json!({"removed": name}))).into_response()
}

// Bring IntoResponse into scope for the `.into_response()` calls above.
use axum::response::IntoResponse;
