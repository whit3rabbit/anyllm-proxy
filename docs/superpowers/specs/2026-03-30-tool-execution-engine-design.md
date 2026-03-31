# Tool Execution Engine Design

**Date:** 2026-03-30
**Status:** Approved
**Scope:** Phase C (wire up existing ToolRegistry + MCP support, single-round default), designed for Phase B (multi-round agentic loop) without architectural changes.

## Problem

anyllm-proxy translates tool calls between Anthropic and OpenAI formats but never executes them. A `Tool` trait, `ToolRegistry`, and two builtin tools (`execute_bash`, `read_file`) exist in `crates/proxy/src/tools/` but are not wired into the request flow. There is no MCP server integration.

LiteLLM's proxy supports single-round MCP tool auto-execution. We aim to match and exceed that by building a bounded execution loop that defaults to single-round but supports multi-round via config.

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Execution location | Handler-level (Approach 2) | Engine called from handlers, direct access to backend client and request context. Avoids middleware buffering issues. |
| Loop model | Bounded loop with `max_iterations` | Default 1 (phase C). Bump to N for phase B. No architecture change needed. |
| Tool policy | Per-tool `Allow`/`Deny`/`PassThrough` | Replaces simple `auto_execute: bool`. Supports mixed execution where some tools run server-side and others pass through to client. |
| Builtin tools | Registered but `PassThrough` by default | `execute_bash` is an RCE vector. Must be explicitly set to `Allow` via config. |
| MCP transport | SSE only (initial) | Covers most remote MCP servers. Stdio/WebSocket added later without architecture change. |
| MCP management | Config file + admin API | Config for initial setup, admin endpoints for runtime add/remove without restart. |

## Architecture

```
Client Request (with tools)
    |
    v
Handler (chat_completions / messages)
    |
    v
ToolExecutionEngine::run(messages, tools, config)
    |
    for i in 0..max_iterations:
    |   +-- Backend Call -> LLM response
    |   +-- Extract tool_use blocks
    |   +-- Partition: auto_execute vs. pass-through
    |   +-- If no auto-execute tools -> break, return response
    |   +-- Execute auto-execute tools (parallel, tokio::JoinSet)
    |   +-- Append assistant message + tool_results to messages
    |   +-- Continue loop
    |
    v
Return to client
    (pass-through tool_use blocks preserved,
     auto-executed results folded into conversation)
```

The engine owns the full loop. Handlers call `engine.run()` once and receive the final response plus a trace.

## Core Types

### ToolExecutionEngine

```rust
pub struct ToolExecutionEngine {
    registry: Arc<ToolRegistry>,
    policy: Arc<ToolExecutionPolicy>,
    backend: Arc<BackendClient>,
}

impl ToolExecutionEngine {
    pub async fn run(
        &self,
        messages: Vec<Message>,
        tools: Vec<Tool>,
        config: LoopConfig,
    ) -> EngineResult { ... }
}
```

### LoopConfig

```rust
pub struct LoopConfig {
    pub max_iterations: usize,          // default: 1
    pub tool_timeout: Duration,         // per-tool execution timeout
    pub total_timeout: Duration,        // wall-clock cap for entire loop
    pub max_tool_calls_per_turn: usize, // cap parallel calls per iteration
}
```

### EngineResult

```rust
pub struct EngineResult {
    pub response: LlmResponse,
    pub trace: LoopTrace,
}
```

## Termination Guards

The loop exits when any condition is met:

1. **No auto-execute tools in response**: normal exit. LLM produced text or only pass-through tool calls.
2. **max_iterations reached**: safety cap prevents runaway loops.
3. **total_timeout exceeded**: wall-clock guard prevents cost explosions.
4. **Duplicate detection**: if the set of (tool_name, arguments) pairs in the current iteration is identical to the previous iteration (compared by value), break. Prevents infinite retry loops where the LLM re-emits the same calls after receiving the same results.
5. **All tool executions failed**: every tool in a turn returned `Error` or `Timeout`. Feeding failures back will likely repeat.

## Execution Policy

```rust
pub struct ToolExecutionPolicy {
    pub default_action: PolicyAction,  // Deny by default
    pub rules: Vec<PolicyRule>,
}

pub struct PolicyRule {
    pub tool_name: String,        // exact or glob pattern
    pub action: PolicyAction,     // Allow, Deny, PassThrough
    pub timeout: Option<Duration>,
    pub max_concurrency: Option<usize>,
}

pub enum PolicyAction {
    Allow,       // auto-execute server-side
    Deny,        // reject the tool call, return error to LLM
    PassThrough, // return to client for execution
}
```

The `auto_execute` config field maps to `Allow`; absence maps to `PassThrough`. Per-tool timeout and concurrency limits override global defaults.

## Failure Modeling

```rust
pub enum ToolOutcome {
    Success(Value),
    Error { message: String, retryable: bool },
    Timeout,
}
```

All variants convert to Anthropic `ToolResult` blocks. `Error` and `Timeout` set `is_error: true` with the error description as content, so the LLM can reason about the failure (retry, try a different tool, or produce a text answer).

## Loop Trace (Observability)

```rust
pub struct LoopTrace {
    pub iterations: Vec<IterationTrace>,
    pub total_duration: Duration,
    pub termination_reason: TerminationReason,
}

pub struct IterationTrace {
    pub tool_calls: Vec<ToolCallTrace>,
    pub llm_latency: Duration,
}

pub struct ToolCallTrace {
    pub tool_name: String,
    pub duration: Duration,
    pub outcome: ToolOutcome,
}

pub enum TerminationReason {
    NoToolCalls,
    MaxIterations,
    Timeout,
    DuplicateDetected,
    AllToolsFailed,
}
```

Returned to the handler for logging, response headers (`x-anyllm-tool-trace`), or export to Langfuse/OTel.

## MCP Integration

### McpServerManager

```rust
pub struct McpServer {
    pub name: String,
    pub url: String,              // SSE endpoint
    pub transport: McpTransport,  // SSE only for now
    pub tools: Vec<McpToolDef>,   // discovered via tools/list
}

pub struct McpServerManager {
    servers: RwLock<HashMap<String, McpServer>>,
    tool_to_server: RwLock<HashMap<String, String>>, // tool_name -> server_name
}
```

### Lifecycle

1. On startup (or admin API call), the manager connects to each configured MCP server.
2. Calls `tools/list` (JSON-RPC) to discover available tools.
3. Populates `tool_to_server` mapping.
4. MCP tools implement the existing `Tool` trait. `execute()` sends a JSON-RPC `tools/call` to the server via SSE.

### Admin Endpoints

- `POST /admin/api/mcp-servers`: Add a server, triggers tool discovery.
- `GET /admin/api/mcp-servers`: List servers and their discovered tools.
- `DELETE /admin/api/mcp-servers/:name`: Remove a server and its tools.

## Configuration

```yaml
# In PROXY_CONFIG (simple format)
tool_execution:
  max_iterations: 1
  tool_timeout_secs: 30
  total_timeout_secs: 300

builtin_tools:
  execute_bash:
    enabled: true
    policy: pass_through    # allow | deny | pass_through
  read_file:
    enabled: true
    policy: pass_through

mcp_servers:
  - name: github
    url: https://mcp.github.com/sse
    policy: allow           # default policy for all tools from this server
```

### Defaults

- `max_iterations`: 1
- `tool_timeout_secs`: 30
- `total_timeout_secs`: 300
- `builtin_tools`: both enabled, both `pass_through`
- `mcp_servers`: empty (no MCP servers configured)

## Streaming

### Phase C (max_iterations=1)

- **Non-streaming**: Engine runs the loop, returns final response. Straightforward.
- **Streaming**: Engine collects the first stream to detect tool calls. If auto-execute tools are found, executes them, makes one follow-up backend call, and streams that response to the client. The client sees a pause during tool execution (same as LiteLLM's `MCPStreamingIterator`).

### Phase B (max_iterations>1, deferred)

Options: buffer each iteration and stream only the final one, or stream every iteration with tool execution events interleaved. Decision deferred to phase B design.

## Crate Placement

| Component | Crate | Rationale |
|-----------|-------|-----------|
| `ToolExecutionEngine` | `crates/proxy` (`src/tools/execution.rs`) | Owns backend client, loop orchestration. |
| `ToolExecutionPolicy` | `crates/proxy` (`src/tools/policy.rs`) | Proxy-level concern, not pure translation. |
| `ToolRegistry`, `Tool` trait | `crates/proxy` (`src/tools/registry.rs`) | Already exists here. |
| Builtin tools | `crates/proxy` (`src/tools/builtin/`) | Already exists here. |
| `McpServerManager` | `crates/proxy` (`src/tools/mcp.rs`) | Proxy-level, requires network IO. |
| Config parsing | `crates/proxy` (`src/config/`) | Extends existing config module. |
| Loop trace types | `crates/proxy` (`src/tools/trace.rs`) | Proxy-level observability. |

No changes to `crates/translator` or `crates/client`. Tool execution is a proxy concern, not a translation concern.

## Phase C vs Phase B Boundary

Phase C (this spec):
- Wire up `ToolRegistry` and builtins into the request flow.
- Implement `ToolExecutionEngine` with bounded loop (default `max_iterations: 1`).
- Add `ToolExecutionPolicy` with `Allow`/`Deny`/`PassThrough`.
- Add `McpServerManager` with SSE transport and admin API.
- Add config parsing for `tool_execution`, `builtin_tools`, `mcp_servers`.
- Failure modeling and loop trace.
- Streaming: collect-then-execute for single round.

Phase B (future, no architecture change needed):
- Bump `max_iterations` default or make it per-request configurable.
- Streaming: interleaved tool execution events.
- Tool result caching and deduplication.
- Per-request `max_iterations` override via header or request field.
- Stdio/WebSocket MCP transports.
