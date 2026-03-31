# Security Audit Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix six security vulnerabilities found in the security audit: arbitrary file read, MCP name ambiguity, add_model input injection, DNS rebinding on MCP client, CSRF improvement, and bash tool hardening.

**Architecture:** All fixes are targeted single-file or two-file changes. No new crates or dependencies required. The read_file fix threads allowed_dirs config through existing config structs. The MCP fix swaps `reqwest::Client::new()` for `build_http_client`. The rest are validation guards.

**Tech Stack:** Rust stable, tokio, reqwest, axum, anyllm_client::http

---

## Task 1: Fix read_file path confinement (Vuln 2 — High)

**Files:**
- Modify: `crates/proxy/src/config/simple.rs` (add `allowed_dirs` to `BuiltinToolConfig`)
- Modify: `crates/proxy/src/tools/builtin/read_file.rs` (add allowed_dirs field, enforce check)
- Modify: `crates/proxy/src/tools/builtin/mod.rs` (pass config to register_all)
- Modify: `crates/proxy/src/main.rs` (pass builtin config to register_all)

- [ ] **Step 1: Add `allowed_dirs` to BuiltinToolConfig in `crates/proxy/src/config/simple.rs`**

Find the `BuiltinToolConfig` struct (around line 114) and add the field:

```rust
/// Configuration for a single builtin tool.
#[derive(Debug, Deserialize)]
pub struct BuiltinToolConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub policy: Option<String>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// For read_file: restrict reads to these absolute directory paths.
    /// If empty or absent, all paths are permitted (dangerous; set this).
    #[serde(default)]
    pub allowed_dirs: Vec<String>,
}
```

- [ ] **Step 2: Add allowed_dirs field to ReadFileTool and enforce it**

Replace the entire `crates/proxy/src/tools/builtin/read_file.rs`:

```rust
use crate::tools::registry::Tool;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Maximum file size to read (1 MB). Prevents OOM from huge files.
const MAX_FILE_SIZE: u64 = 1024 * 1024;

/// Tool for reading file contents safely.
pub struct ReadFileTool {
    /// If non-empty, restrict reads to files under these directories.
    /// All entries must be canonical absolute paths (no symlinks, no ..).
    pub allowed_dirs: Vec<PathBuf>,
}

impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Reads the contents of a local file and returns it as a string."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The absolute path to the file to read."
                }
            },
            "required": ["path"]
        })
    }

    fn execute<'a>(
        &'a self,
        input: Value,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, String>> + Send + 'a>>
    {
        let allowed_dirs = self.allowed_dirs.clone();
        Box::pin(async move {
            let raw_path = input
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing 'path' argument".to_string())?;

            let path = Path::new(raw_path);

            // Require absolute paths to prevent relative path traversal.
            if !path.is_absolute() {
                return Err("Only absolute paths are allowed".to_string());
            }

            // Resolve symlinks and .. components.
            let canonical = path
                .canonicalize()
                .map_err(|e| format!("Cannot resolve path '{}': {}", raw_path, e))?;

            // Enforce allowed_dirs allowlist. If configured, the canonical path
            // must start with at least one of the allowed directory prefixes.
            if !allowed_dirs.is_empty() {
                let permitted = allowed_dirs
                    .iter()
                    .any(|base| canonical.starts_with(base));
                if !permitted {
                    return Err(format!(
                        "Path '{}' is outside the configured allowed directories",
                        raw_path
                    ));
                }
            } else {
                // No allowed_dirs configured: log a warning. Operator should set this.
                tracing::warn!(
                    path = %raw_path,
                    "read_file executed with no allowed_dirs restriction; \
                     set allowed_dirs in builtin_tools config to restrict access"
                );
            }

            // Check file size before reading.
            let metadata = std::fs::metadata(&canonical)
                .map_err(|e| format!("Cannot stat '{}': {}", raw_path, e))?;
            if metadata.len() > MAX_FILE_SIZE {
                return Err(format!(
                    "File is {} bytes, exceeds {} byte limit",
                    metadata.len(),
                    MAX_FILE_SIZE
                ));
            }

            match std::fs::read_to_string(&canonical) {
                Ok(content) => Ok(serde_json::json!({ "content": content })),
                Err(e) => Err(format!("Failed to read file '{}': {}", raw_path, e)),
            }
        })
    }
}
```

- [ ] **Step 3: Update `register_all` to accept config and pass allowed_dirs**

Replace `crates/proxy/src/tools/builtin/mod.rs`:

```rust
// SAFETY: The tools in this module execute arbitrary shell commands (BashTool)
// and read arbitrary files (ReadFileTool) as the proxy process user. They must
// NEVER be registered in a server-side tool registry without explicit operator
// opt-in and appropriate sandboxing. Gated behind the `dangerous-builtin-tools`
// feature flag, which is OFF by default.

#[cfg(feature = "dangerous-builtin-tools")]
pub mod bash;
#[cfg(feature = "dangerous-builtin-tools")]
pub mod read_file;

use crate::tools::registry::ToolRegistry;

/// Populate a registry with standard built-in tools.
///
/// `builtin_configs`: map of tool name -> config from PROXY_CONFIG; used to pass
/// per-tool settings (e.g., `allowed_dirs` for `read_file`) to tool constructors.
///
/// # Safety
///
/// When `dangerous-builtin-tools` is enabled, this registers `BashTool` (arbitrary
/// shell execution) and `ReadFileTool` (arbitrary file reads). Only call this if
/// the tool execution engine is sandboxed or if the operator has explicitly opted in.
///
/// When the feature is disabled (the default), this is a no-op.
pub fn register_all(
    _registry: &mut ToolRegistry,
    _builtin_configs: Option<&std::collections::HashMap<String, crate::config::simple::BuiltinToolConfig>>,
) {
    #[cfg(feature = "dangerous-builtin-tools")]
    {
        _registry.register(Box::new(bash::BashTool));

        // Build ReadFileTool with allowed_dirs from config, if present.
        let allowed_dirs = _builtin_configs
            .and_then(|m| m.get("read_file"))
            .map(|cfg| {
                cfg.allowed_dirs
                    .iter()
                    .filter_map(|d| {
                        let p = std::path::PathBuf::from(d);
                        // Canonicalize at registration time so we compare canonical paths.
                        match p.canonicalize() {
                            Ok(canon) => Some(canon),
                            Err(e) => {
                                tracing::warn!(
                                    dir = %d,
                                    error = %e,
                                    "read_file allowed_dirs entry could not be canonicalized; skipping"
                                );
                                None
                            }
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        _registry.register(Box::new(read_file::ReadFileTool { allowed_dirs }));
    }
}
```

- [ ] **Step 4: Update the call site in `crates/proxy/src/main.rs`**

Find the line `anyllm_proxy::tools::builtin::register_all(&mut registry);` (around line 178) and change it to:

```rust
anyllm_proxy::tools::builtin::register_all(
    &mut registry,
    simple_config_shell.builtin_tools.as_ref(),
);
```

- [ ] **Step 5: Run tests to verify nothing breaks**

```bash
cd /Users/whit3rabbit/Documents/GitHub/llm-translate-api
cargo test -p anyllm_proxy 2>&1 | tail -20
```

Expected: all tests pass, no compile errors.

- [ ] **Step 6: Commit**

```bash
git add crates/proxy/src/config/simple.rs \
        crates/proxy/src/tools/builtin/read_file.rs \
        crates/proxy/src/tools/builtin/mod.rs \
        crates/proxy/src/main.rs
git commit -m "fix(tools): enforce allowed_dirs allowlist in ReadFileTool

Adds allowed_dirs config field to BuiltinToolConfig. ReadFileTool now
rejects reads outside the configured base directories after canonicalize().
Logs a warning when allowed_dirs is empty. Threads config through
register_all so tool constructors receive per-tool settings."
```

---

## Task 2: Fix MCP server name ambiguity (Vuln 3 — High)

**Files:**
- Modify: `crates/proxy/src/tools/mcp.rs` (validate server name, reject underscores)

- [ ] **Step 1: Add `is_valid_mcp_server_name` and return Result from `register_server_blocking`**

In `crates/proxy/src/tools/mcp.rs`, add a validation function and change the signature:

```rust
/// Validate MCP server name: must be non-empty, alphanumeric + hyphens only.
/// Underscores are forbidden because the tool name scheme uses `mcp_{server}_{tool}`;
/// an underscore in the server name makes `parse_mcp_tool_name` ambiguous.
pub fn is_valid_mcp_server_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-')
}
```

Change `register_server_blocking` to return `Result<(), String>`:

```rust
pub fn register_server_blocking(
    &self,
    name: &str,
    url: &str,
    tools: Vec<McpToolDef>,
) -> Result<(), String> {
    if !is_valid_mcp_server_name(name) {
        return Err(format!(
            "invalid MCP server name '{}': only alphanumerics and hyphens allowed",
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
    self.servers.write().unwrap().insert(name.to_string(), server);
    Ok(())
}
```

- [ ] **Step 2: Update all callers of `register_server_blocking`**

In `crates/proxy/src/main.rs` (around line 187-200), the call to `manager.register_server_blocking(...)` must handle the Result:

```rust
if let Err(e) = manager.register_server_blocking(
    &server_cfg.name,
    &server_cfg.url,
    tools,
) {
    tracing::error!(
        server = %server_cfg.name,
        error = %e,
        "MCP server registration failed"
    );
    continue;
}
```

In `crates/proxy/src/admin/routes.rs`, the handler for `POST /admin/api/mcp-servers` also calls `register_server_blocking`. Find it and handle the Result:

```rust
if let Err(e) = mcp_manager.register_server_blocking(&body.name, &body.url, tools) {
    return (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": e})),
    ).into_response();
}
```

- [ ] **Step 3: Update tests in mcp.rs that call register_server_blocking**

Find all test calls like `mgr.register_server_blocking("github", ...)` and append `.unwrap()` or `.expect(...)` since they use valid names and should succeed.

- [ ] **Step 4: Run tests**

```bash
cargo test -p anyllm_proxy 2>&1 | tail -20
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/proxy/src/tools/mcp.rs crates/proxy/src/main.rs crates/proxy/src/admin/routes.rs
git commit -m "fix(mcp): validate server names to prevent tool routing ambiguity

MCP tool names use mcp_{server}_{tool} scheme; underscores in server names
cause parse_mcp_tool_name to misroute calls. is_valid_mcp_server_name now
rejects names containing underscores. register_server_blocking returns
Result<(), String> so callers can handle invalid names at registration time."
```

---

## Task 3: Fix add_model input validation (Vuln 5 — Medium)

**Files:**
- Modify: `crates/proxy/src/admin/routes.rs` (add is_safe_model_name checks to add_model)

- [ ] **Step 1: Add validation at the top of the `add_model` handler**

In `crates/proxy/src/admin/routes.rs`, find the `add_model` function (around line 1449) and add validation after the model router guard:

```rust
async fn add_model(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(shared): State<SharedState>,
    Json(body): Json<AddModelRequest>,
) -> impl IntoResponse {
    let Some(ref router_lock) = shared.model_router else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "no model router active"})),
        )
            .into_response();
    };

    // Validate all name fields to prevent log injection and routing issues.
    for (field, value) in [
        ("model_name", &body.model_name),
        ("backend_name", &body.backend_name),
        ("actual_model", &body.actual_model),
    ] {
        if !is_safe_model_name(value) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("invalid {field}: contains disallowed characters")
                })),
            )
                .into_response();
        }
    }

    // ... rest of the existing function unchanged
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p anyllm_proxy 2>&1 | tail -20
```

Expected: all tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/proxy/src/admin/routes.rs
git commit -m "fix(admin): validate model_name/backend_name/actual_model in add_model

Applies is_safe_model_name to all three AddModelRequest fields before use,
preventing log injection via newlines or control characters in audit log
detail entries. Consistent with existing validation in put_config."
```

---

## Task 4: Fix MCP DNS rebinding via SSRF-safe HTTP client (Vuln 6 — Medium)

**Files:**
- Modify: `crates/proxy/src/tools/mcp.rs` (replace reqwest::Client::new() with build_http_client)

- [ ] **Step 1: Replace the plain reqwest client in McpServerManager with an SSRF-safe client**

In `crates/proxy/src/tools/mcp.rs`, update the import and `McpServerManager::new()`:

At the top of the file, add the import:
```rust
use anyllm_client::http::{build_http_client, HttpClientConfig};
```

Change `McpServerManager::new()`:
```rust
pub fn new() -> Self {
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
```

Also fix `discover_tools` (the static method that creates its own client):
```rust
pub async fn discover_tools(url: &str) -> Result<Vec<McpToolDef>, String> {
    let client = build_http_client(&HttpClientConfig {
        ssrf_protection: true,
        ..Default::default()
    });
    discover_tools_impl(&client, url).await
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p anyllm_proxy 2>&1 | tail -20
```

Expected: all tests pass. The SSRF-safe resolver is a no-op for the mock URLs used in unit tests.

- [ ] **Step 3: Commit**

```bash
git add crates/proxy/src/tools/mcp.rs
git commit -m "fix(mcp): use SSRF-safe HTTP client for MCP tool calls

Replaces reqwest::Client::new() in McpServerManager with build_http_client
(ssrf_protection: true), which attaches SsrfSafeDnsResolver. This prevents
DNS rebinding: a domain that passes the registration-time check but later
resolves to a private/metadata IP (e.g. 169.254.169.254) will be blocked
at connection time by the resolver."
```

---

## Task 5: CSRF one-time token tracking (Vuln 4 — Medium)

**Files:**
- Modify: `crates/proxy/src/admin/state.rs` (add issued_csrf_tokens DashMap)
- Modify: `crates/proxy/src/admin/routes.rs` (store token on issue, remove on use)

- [ ] **Step 1: Check state.rs structure**

```bash
grep -n "struct AdminState\|SharedState\|DashMap\|csrf" \
  crates/proxy/src/admin/state.rs | head -20
```

- [ ] **Step 2: Add `issued_csrf_tokens` to admin state**

In `crates/proxy/src/admin/state.rs`, find the `AdminState` or `SharedState` struct. Add a field:

```rust
/// Set of CSRF tokens issued by GET /admin/csrf-token.
/// Tokens are removed on first successful use (one-time tokens).
/// Bounded by MaxAge=86400; background cleanup not strictly needed for
/// localhost-only admin, but the set stays small in practice.
pub issued_csrf_tokens: Arc<DashMap<String, ()>>,
```

Also update the constructor / `Default` impl to initialize the new field:
```rust
issued_csrf_tokens: Arc::new(DashMap::new()),
```

- [ ] **Step 3: Store token on issue in `get_csrf_token`**

In `crates/proxy/src/admin/routes.rs`, find `get_csrf_token()`. After generating the token, insert it into the shared set. This requires the handler to accept `State(shared): State<SharedState>`:

```rust
async fn get_csrf_token(State(shared): State<SharedState>) -> axum::response::Response {
    let token = generate_csrf_token();
    shared.issued_csrf_tokens.insert(token.clone(), ());
    let body = serde_json::json!({"csrf_token": token});
    axum::http::Response::builder()
        // ... existing headers unchanged
```

- [ ] **Step 4: Validate and consume the token in the CSRF middleware**

In `crates/proxy/src/admin/routes.rs`, find the CSRF validation middleware (the function that calls `validate_csrf_tokens`). After the `validate_csrf_tokens` check passes, also verify the token was server-issued and remove it (one-time use):

```rust
// Verify the token was server-issued (prevents forgery).
// Remove it immediately to enforce one-time use.
if shared.issued_csrf_tokens.remove(csrf_header).is_none() {
    return (
        StatusCode::FORBIDDEN,
        axum::Json(serde_json::json!({
            "error": {"type": "forbidden", "message": "CSRF token not recognized or already used"}
        })),
    ).into_response();
}
```

- [ ] **Step 5: Run tests**

```bash
cargo test -p anyllm_proxy 2>&1 | tail -20
```

Expected: all tests pass. Any tests that exercise CSRF must now call the csrf-token endpoint first.

- [ ] **Step 6: Commit**

```bash
git add crates/proxy/src/admin/state.rs crates/proxy/src/admin/routes.rs
git commit -m "fix(admin): enforce one-time CSRF tokens tracked server-side

GET /admin/csrf-token now inserts the generated token into a DashMap on
SharedState. CSRF validation middleware checks the token was server-issued
and removes it on first use, preventing replay of previously issued tokens.
This closes the window where any localhost process could prefetch a token
and reuse it across multiple requests."
```

---

## Task 6: BashTool startup warning (Vuln 1 — High, design-level)

**Files:**
- Modify: `crates/proxy/src/tools/builtin/mod.rs` (add warning when bash is allow-listed)
- Modify: `crates/proxy/src/config/simple.rs` (emit warn in build_tool_config)

- [ ] **Step 1: Add a startup warning when execute_bash policy is `allow`**

In `crates/proxy/src/config/simple.rs`, in the `build_tool_config` method, after the `action` is determined for a tool named `execute_bash` with `PolicyAction::Allow`, emit:

```rust
if name == "execute_bash" && action == PolicyAction::Allow {
    tracing::warn!(
        "execute_bash policy is set to Allow. This permits the LLM to execute \
         arbitrary OS commands as the proxy process user. Ensure the proxy runs \
         in an isolated environment (seccomp, read-only rootfs, no network access \
         from the sandbox) before enabling this in production."
    );
}
```

- [ ] **Step 2: Run tests and build**

```bash
cargo build 2>&1 | tail -10
cargo test -p anyllm_proxy 2>&1 | tail -10
```

Expected: clean build, tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/proxy/src/config/simple.rs
git commit -m "fix(tools): warn at startup when execute_bash policy is Allow

Emits a tracing::warn! when execute_bash is configured with policy: allow
so operators see an explicit reminder that the tool executes arbitrary OS
commands. The dangerous-builtin-tools feature flag remains the primary
compile-time gate; this is an additional runtime visibility measure."
```

---

## Final verification

- [ ] **Run full test suite**

```bash
cargo test 2>&1 | tail -30
```

Expected: ~906+ tests pass, 8 ignored, 0 failures.

- [ ] **Run clippy**

```bash
cargo clippy -- -D warnings 2>&1 | tail -20
```

Expected: no warnings.
