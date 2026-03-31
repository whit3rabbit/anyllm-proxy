# Tool Execution Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire up the existing `ToolRegistry` and builtin tools into the request flow, add a bounded tool execution loop with policy enforcement, and integrate MCP server support via SSE transport with admin API management.

**Architecture:** Handler-level execution engine (`ToolExecutionEngine`) owns the full LLM-tool loop. Handlers call `engine.run()` and get back the final response plus an observability trace. Tools are partitioned per-call into `Allow` (auto-execute), `Deny` (reject), or `PassThrough` (return to client). MCP servers are managed via config + admin API, with tool discovery on connect.

**Tech Stack:** Rust, axum, tokio, reqwest, serde, serde_json, serde_yaml (existing dep). MCP over SSE (reqwest + eventsource-stream or manual SSE parsing). No new heavyweight dependencies.

**Spec:** `docs/superpowers/specs/2026-03-30-tool-execution-engine-design.md`

---

## File Structure

| File | Responsibility | Status |
|------|---------------|--------|
| `crates/proxy/src/tools/mod.rs` | Module declarations, re-exports | Modify |
| `crates/proxy/src/tools/registry.rs` | `Tool` trait, `ToolRegistry` | Modify (add `list_names()`) |
| `crates/proxy/src/tools/policy.rs` | `ToolExecutionPolicy`, `PolicyAction`, `PolicyRule` | Create |
| `crates/proxy/src/tools/trace.rs` | `LoopTrace`, `IterationTrace`, `ToolCallTrace`, `TerminationReason` | Create |
| `crates/proxy/src/tools/execution.rs` | `ToolExecutionEngine`, `LoopConfig`, `EngineResult`, `ToolOutcome` | Create |
| `crates/proxy/src/tools/mcp.rs` | `McpServerManager`, `McpServer`, `McpTool`, SSE transport | Create |
| `crates/proxy/src/tools/builtin/bash.rs` | `BashTool` | Existing (no changes) |
| `crates/proxy/src/tools/builtin/read_file.rs` | `ReadFileTool` | Existing (no changes) |
| `crates/proxy/src/config/simple.rs` | Parse `tool_execution`, `builtin_tools`, `mcp_servers` config sections | Modify |
| `crates/proxy/src/main.rs` | Build `ToolExecutionEngine`, add to `AppState` | Modify |
| `crates/proxy/src/server/routes.rs` | Add `tool_engine` field to `AppState`, wire into handlers | Modify |
| `crates/proxy/src/server/chat_completions.rs` | Call engine for non-streaming tool execution | Modify |
| `crates/proxy/src/admin/routes.rs` | Add MCP server admin endpoints | Modify |

---

### Task 1: Policy Types

**Files:**
- Create: `crates/proxy/src/tools/policy.rs`
- Modify: `crates/proxy/src/tools/mod.rs`

- [ ] **Step 1: Write the failing test**

In `crates/proxy/src/tools/policy.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_denies() {
        let policy = ToolExecutionPolicy::default();
        assert_eq!(policy.resolve("anything"), PolicyAction::PassThrough);
    }

    #[test]
    fn exact_match_rule() {
        let policy = ToolExecutionPolicy {
            default_action: PolicyAction::PassThrough,
            rules: vec![PolicyRule {
                tool_name: "read_file".to_string(),
                action: PolicyAction::Allow,
                timeout: None,
                max_concurrency: None,
            }],
        };
        assert_eq!(policy.resolve("read_file"), PolicyAction::Allow);
        assert_eq!(policy.resolve("execute_bash"), PolicyAction::PassThrough);
    }

    #[test]
    fn glob_pattern_rule() {
        let policy = ToolExecutionPolicy {
            default_action: PolicyAction::PassThrough,
            rules: vec![PolicyRule {
                tool_name: "mcp_github_*".to_string(),
                action: PolicyAction::Allow,
                timeout: None,
                max_concurrency: None,
            }],
        };
        assert_eq!(policy.resolve("mcp_github_search"), PolicyAction::Allow);
        assert_eq!(policy.resolve("mcp_slack_send"), PolicyAction::PassThrough);
    }

    #[test]
    fn deny_action_blocks_tool() {
        let policy = ToolExecutionPolicy {
            default_action: PolicyAction::Allow,
            rules: vec![PolicyRule {
                tool_name: "execute_bash".to_string(),
                action: PolicyAction::Deny,
                timeout: None,
                max_concurrency: None,
            }],
        };
        assert_eq!(policy.resolve("execute_bash"), PolicyAction::Deny);
        assert_eq!(policy.resolve("read_file"), PolicyAction::Allow);
    }

    #[test]
    fn timeout_override_per_tool() {
        let policy = ToolExecutionPolicy {
            default_action: PolicyAction::PassThrough,
            rules: vec![PolicyRule {
                tool_name: "slow_tool".to_string(),
                action: PolicyAction::Allow,
                timeout: Some(std::time::Duration::from_secs(60)),
                max_concurrency: None,
            }],
        };
        let rule = policy.find_rule("slow_tool").unwrap();
        assert_eq!(rule.timeout, Some(std::time::Duration::from_secs(60)));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p anyllm_proxy policy::tests -- -v`
Expected: FAIL (module does not exist yet)

- [ ] **Step 3: Write the implementation**

In `crates/proxy/src/tools/policy.rs`:

```rust
use std::time::Duration;

/// Action to take when a tool call is encountered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyAction {
    /// Execute the tool server-side automatically.
    Allow,
    /// Reject the tool call and return an error to the LLM.
    Deny,
    /// Pass the tool call through to the client for execution.
    PassThrough,
}

/// A rule matching a tool by name (exact or glob with trailing `*`).
#[derive(Debug, Clone)]
pub struct PolicyRule {
    pub tool_name: String,
    pub action: PolicyAction,
    pub timeout: Option<Duration>,
    pub max_concurrency: Option<usize>,
}

impl PolicyRule {
    fn matches(&self, name: &str) -> bool {
        if let Some(prefix) = self.tool_name.strip_suffix('*') {
            name.starts_with(prefix)
        } else {
            self.tool_name == name
        }
    }
}

/// Policy that determines how each tool call is handled.
#[derive(Debug, Clone)]
pub struct ToolExecutionPolicy {
    pub default_action: PolicyAction,
    pub rules: Vec<PolicyRule>,
}

impl ToolExecutionPolicy {
    /// Resolve the action for a given tool name. First matching rule wins.
    pub fn resolve(&self, tool_name: &str) -> PolicyAction {
        for rule in &self.rules {
            if rule.matches(tool_name) {
                return rule.action;
            }
        }
        self.default_action
    }

    /// Find the first matching rule for a tool name (for timeout/concurrency overrides).
    pub fn find_rule(&self, tool_name: &str) -> Option<&PolicyRule> {
        self.rules.iter().find(|r| r.matches(tool_name))
    }
}

impl Default for ToolExecutionPolicy {
    fn default() -> Self {
        Self {
            default_action: PolicyAction::PassThrough,
            rules: Vec::new(),
        }
    }
}
```

- [ ] **Step 4: Update mod.rs**

In `crates/proxy/src/tools/mod.rs`, replace the entire file:

```rust
// Tool execution engine and registry

pub mod builtin;
pub mod policy;
pub mod registry;

pub use policy::{PolicyAction, PolicyRule, ToolExecutionPolicy};
pub use registry::{Tool, ToolRegistry};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p anyllm_proxy policy::tests -- -v`
Expected: 5 tests PASS

- [ ] **Step 6: Commit**

```bash
git add crates/proxy/src/tools/policy.rs crates/proxy/src/tools/mod.rs
git commit -m "feat(tools): add ToolExecutionPolicy with Allow/Deny/PassThrough actions"
```

---

### Task 2: Trace Types

**Files:**
- Create: `crates/proxy/src/tools/trace.rs`
- Modify: `crates/proxy/src/tools/mod.rs`

- [ ] **Step 1: Write the failing test**

In `crates/proxy/src/tools/trace.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn empty_trace_reports_no_tool_calls() {
        let trace = LoopTrace {
            iterations: vec![],
            total_duration: Duration::from_millis(100),
            termination_reason: TerminationReason::NoToolCalls,
        };
        assert_eq!(trace.total_tool_calls(), 0);
        assert_eq!(trace.iterations.len(), 0);
    }

    #[test]
    fn trace_counts_tool_calls_across_iterations() {
        let trace = LoopTrace {
            iterations: vec![
                IterationTrace {
                    tool_calls: vec![
                        ToolCallTrace {
                            tool_name: "read_file".to_string(),
                            duration: Duration::from_millis(50),
                            outcome: ToolOutcome::Success(serde_json::json!({"content": "hi"})),
                        },
                    ],
                    llm_latency: Duration::from_millis(200),
                },
                IterationTrace {
                    tool_calls: vec![
                        ToolCallTrace {
                            tool_name: "read_file".to_string(),
                            duration: Duration::from_millis(30),
                            outcome: ToolOutcome::Error {
                                message: "not found".to_string(),
                                retryable: false,
                            },
                        },
                    ],
                    llm_latency: Duration::from_millis(150),
                },
            ],
            total_duration: Duration::from_millis(430),
            termination_reason: TerminationReason::AllToolsFailed,
        };
        assert_eq!(trace.total_tool_calls(), 2);
    }

    #[test]
    fn tool_outcome_serializes_to_json() {
        let success = ToolOutcome::Success(serde_json::json!({"result": 42}));
        let json = serde_json::to_value(&success).unwrap();
        assert_eq!(json["type"], "success");

        let error = ToolOutcome::Error {
            message: "fail".to_string(),
            retryable: true,
        };
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["type"], "error");
        assert_eq!(json["retryable"], true);

        let timeout = ToolOutcome::Timeout;
        let json = serde_json::to_value(&timeout).unwrap();
        assert_eq!(json["type"], "timeout");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p anyllm_proxy trace::tests -- -v`
Expected: FAIL (module does not exist)

- [ ] **Step 3: Write the implementation**

In `crates/proxy/src/tools/trace.rs`:

```rust
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

/// Outcome of a single tool execution.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolOutcome {
    Success(Value),
    Error { message: String, retryable: bool },
    Timeout,
}

/// Why the execution loop terminated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminationReason {
    /// LLM returned no auto-executable tool calls.
    NoToolCalls,
    /// Hit the max_iterations cap.
    MaxIterations,
    /// Wall-clock timeout exceeded.
    Timeout,
    /// Same tool calls with identical arguments as previous iteration.
    DuplicateDetected,
    /// Every tool in the turn failed or timed out.
    AllToolsFailed,
}

/// Trace of a single tool call within an iteration.
#[derive(Debug, Clone, Serialize)]
pub struct ToolCallTrace {
    pub tool_name: String,
    pub duration: Duration,
    pub outcome: ToolOutcome,
}

/// Trace of a single iteration of the execution loop.
#[derive(Debug, Clone, Serialize)]
pub struct IterationTrace {
    pub tool_calls: Vec<ToolCallTrace>,
    pub llm_latency: Duration,
}

/// Full trace of the execution loop.
#[derive(Debug, Clone, Serialize)]
pub struct LoopTrace {
    pub iterations: Vec<IterationTrace>,
    pub total_duration: Duration,
    pub termination_reason: TerminationReason,
}

impl LoopTrace {
    /// Total number of tool calls across all iterations.
    pub fn total_tool_calls(&self) -> usize {
        self.iterations.iter().map(|i| i.tool_calls.len()).sum()
    }
}
```

- [ ] **Step 4: Update mod.rs**

In `crates/proxy/src/tools/mod.rs`, add `pub mod trace;` and re-export:

```rust
// Tool execution engine and registry

pub mod builtin;
pub mod policy;
pub mod registry;
pub mod trace;

pub use policy::{PolicyAction, PolicyRule, ToolExecutionPolicy};
pub use registry::{Tool, ToolRegistry};
pub use trace::{LoopTrace, TerminationReason, ToolCallTrace, ToolOutcome};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p anyllm_proxy trace::tests -- -v`
Expected: 3 tests PASS

- [ ] **Step 6: Commit**

```bash
git add crates/proxy/src/tools/trace.rs crates/proxy/src/tools/mod.rs
git commit -m "feat(tools): add LoopTrace, ToolOutcome, and TerminationReason types"
```

---

### Task 3: ToolExecutionEngine Core (Non-Streaming)

**Files:**
- Create: `crates/proxy/src/tools/execution.rs`
- Modify: `crates/proxy/src/tools/mod.rs`
- Modify: `crates/proxy/src/tools/registry.rs` (add `list_names()`)

- [ ] **Step 1: Add `list_names()` to ToolRegistry**

In `crates/proxy/src/tools/registry.rs`, add inside `impl ToolRegistry`:

```rust
    /// List all registered tool names.
    pub fn list_names(&self) -> Vec<&str> {
        self.tools.keys().map(|s| s.as_str()).collect()
    }
```

- [ ] **Step 2: Write the failing test for the execution engine**

In `crates/proxy/src/tools/execution.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::policy::{PolicyAction, ToolExecutionPolicy};
    use crate::tools::trace::TerminationReason;

    /// A simple test tool that returns its input uppercased.
    struct EchoTool;

    impl crate::tools::Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "Echoes input"
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string"}
                },
                "required": ["text"]
            })
        }
        fn execute<'a>(
            &'a self,
            input: serde_json::Value,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<serde_json::Value, String>> + Send + 'a>>
        {
            Box::pin(async move {
                let text = input["text"].as_str().unwrap_or("").to_uppercase();
                Ok(serde_json::json!({"result": text}))
            })
        }
    }

    /// A tool that always fails.
    struct FailTool;

    impl crate::tools::Tool for FailTool {
        fn name(&self) -> &str {
            "fail_tool"
        }
        fn description(&self) -> &str {
            "Always fails"
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        fn execute<'a>(
            &'a self,
            _input: serde_json::Value,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<serde_json::Value, String>> + Send + 'a>>
        {
            Box::pin(async { Err("always fails".to_string()) })
        }
    }

    fn make_registry_with_echo() -> ToolRegistry {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(EchoTool));
        reg
    }

    fn make_registry_with_fail() -> ToolRegistry {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(FailTool));
        reg
    }

    fn allow_all_policy() -> ToolExecutionPolicy {
        ToolExecutionPolicy {
            default_action: PolicyAction::Allow,
            rules: vec![],
        }
    }

    fn passthrough_policy() -> ToolExecutionPolicy {
        ToolExecutionPolicy::default()
    }

    #[test]
    fn partition_no_auto_execute() {
        let policy = passthrough_policy();
        let registry = make_registry_with_echo();
        let tool_calls = vec![
            ToolCall {
                id: "1".to_string(),
                name: "echo".to_string(),
                input: serde_json::json!({"text": "hi"}),
            },
        ];
        let (auto, pass) = partition_tool_calls(&tool_calls, &registry, &policy);
        assert!(auto.is_empty());
        assert_eq!(pass.len(), 1);
    }

    #[test]
    fn partition_with_allow_policy() {
        let policy = allow_all_policy();
        let registry = make_registry_with_echo();
        let tool_calls = vec![
            ToolCall {
                id: "1".to_string(),
                name: "echo".to_string(),
                input: serde_json::json!({"text": "hi"}),
            },
            ToolCall {
                id: "2".to_string(),
                name: "unknown_tool".to_string(),
                input: serde_json::json!({}),
            },
        ];
        let (auto, pass) = partition_tool_calls(&tool_calls, &registry, &policy);
        // "echo" is in registry and policy=Allow -> auto-execute
        assert_eq!(auto.len(), 1);
        assert_eq!(auto[0].name, "echo");
        // "unknown_tool" is not in registry -> pass-through regardless of policy
        assert_eq!(pass.len(), 1);
        assert_eq!(pass[0].name, "unknown_tool");
    }

    #[tokio::test]
    async fn execute_tools_parallel_success() {
        let registry = Arc::new(make_registry_with_echo());
        let policy = Arc::new(allow_all_policy());
        let config = LoopConfig::default();
        let calls = vec![
            ToolCall {
                id: "1".to_string(),
                name: "echo".to_string(),
                input: serde_json::json!({"text": "hello"}),
            },
        ];
        let results = execute_tool_calls(&calls, &registry, &policy, &config).await;
        assert_eq!(results.len(), 1);
        match &results[0].outcome {
            ToolOutcome::Success(v) => assert_eq!(v["result"], "HELLO"),
            other => panic!("expected Success, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn execute_tools_parallel_failure() {
        let registry = Arc::new(make_registry_with_fail());
        let policy = Arc::new(allow_all_policy());
        let config = LoopConfig::default();
        let calls = vec![
            ToolCall {
                id: "1".to_string(),
                name: "fail_tool".to_string(),
                input: serde_json::json!({}),
            },
        ];
        let results = execute_tool_calls(&calls, &registry, &policy, &config).await;
        assert_eq!(results.len(), 1);
        match &results[0].outcome {
            ToolOutcome::Error { message, .. } => assert!(message.contains("always fails")),
            other => panic!("expected Error, got {:?}", other),
        }
    }

    #[test]
    fn duplicate_detection_identifies_same_calls() {
        let a = vec![
            ToolCall {
                id: "1".to_string(),
                name: "echo".to_string(),
                input: serde_json::json!({"text": "hi"}),
            },
        ];
        let b = vec![
            ToolCall {
                id: "99".to_string(), // different ID, same name+input
                name: "echo".to_string(),
                input: serde_json::json!({"text": "hi"}),
            },
        ];
        assert!(is_duplicate(&a, &b));
    }

    #[test]
    fn duplicate_detection_different_args() {
        let a = vec![
            ToolCall {
                id: "1".to_string(),
                name: "echo".to_string(),
                input: serde_json::json!({"text": "hi"}),
            },
        ];
        let b = vec![
            ToolCall {
                id: "2".to_string(),
                name: "echo".to_string(),
                input: serde_json::json!({"text": "bye"}),
            },
        ];
        assert!(!is_duplicate(&a, &b));
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p anyllm_proxy execution::tests -- -v`
Expected: FAIL (module does not exist)

- [ ] **Step 4: Write the implementation**

In `crates/proxy/src/tools/execution.rs`:

```rust
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::tools::policy::{PolicyAction, ToolExecutionPolicy};
use crate::tools::registry::ToolRegistry;
use crate::tools::trace::{
    IterationTrace, LoopTrace, TerminationReason, ToolCallTrace, ToolOutcome,
};

/// A tool call extracted from an LLM response.
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

/// Result of a single tool execution, tied back to the original call.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub tool_use_id: String,
    pub tool_name: String,
    pub outcome: ToolOutcome,
}

/// Configuration for the execution loop.
#[derive(Debug, Clone)]
pub struct LoopConfig {
    /// Maximum number of LLM round-trips. Default: 1.
    pub max_iterations: usize,
    /// Per-tool execution timeout. Default: 30s.
    pub tool_timeout: Duration,
    /// Wall-clock cap for the entire loop. Default: 300s.
    pub total_timeout: Duration,
    /// Max parallel tool calls per iteration. Default: 16.
    pub max_tool_calls_per_turn: usize,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            max_iterations: 1,
            tool_timeout: Duration::from_secs(30),
            total_timeout: Duration::from_secs(300),
            max_tool_calls_per_turn: 16,
        }
    }
}

/// Partition tool calls into (auto_execute, pass_through) based on policy and registry.
/// A tool is auto-executed only if it exists in the registry AND the policy says Allow.
/// Denied tools produce immediate error results (handled by the caller).
pub fn partition_tool_calls<'a>(
    tool_calls: &'a [ToolCall],
    registry: &ToolRegistry,
    policy: &ToolExecutionPolicy,
) -> (Vec<&'a ToolCall>, Vec<&'a ToolCall>) {
    let mut auto_execute = Vec::new();
    let mut pass_through = Vec::new();

    for call in tool_calls {
        if !registry.contains(&call.name) {
            // Tool not in registry: always pass through, regardless of policy.
            pass_through.push(call);
            continue;
        }
        match policy.resolve(&call.name) {
            PolicyAction::Allow => auto_execute.push(call),
            PolicyAction::Deny => {
                // Denied tools are treated as pass-through with an error.
                // The caller converts these to ToolResult with is_error=true.
                pass_through.push(call);
            }
            PolicyAction::PassThrough => pass_through.push(call),
        }
    }

    (auto_execute, pass_through)
}

/// Execute tool calls in parallel using tokio::JoinSet.
/// Returns a ToolResult for each call, preserving order.
pub async fn execute_tool_calls(
    calls: &[ToolCall],
    registry: &Arc<ToolRegistry>,
    policy: &Arc<ToolExecutionPolicy>,
    config: &LoopConfig,
) -> Vec<ToolResult> {
    use tokio::task::JoinSet;

    let mut set = JoinSet::new();

    // Cap the number of parallel calls.
    let calls_to_run = if calls.len() > config.max_tool_calls_per_turn {
        tracing::warn!(
            requested = calls.len(),
            cap = config.max_tool_calls_per_turn,
            "capping parallel tool calls"
        );
        &calls[..config.max_tool_calls_per_turn]
    } else {
        calls
    };

    for (idx, call) in calls_to_run.iter().enumerate() {
        let registry = registry.clone();
        let call_id = call.id.clone();
        let call_name = call.name.clone();
        let call_input = call.input.clone();
        let timeout = policy
            .find_rule(&call.name)
            .and_then(|r| r.timeout)
            .unwrap_or(config.tool_timeout);

        set.spawn(async move {
            let start = Instant::now();
            let outcome =
                match tokio::time::timeout(timeout, execute_single(&registry, &call_name, call_input)).await {
                    Ok(Ok(value)) => ToolOutcome::Success(value),
                    Ok(Err(msg)) => ToolOutcome::Error {
                        message: msg,
                        retryable: false,
                    },
                    Err(_) => ToolOutcome::Timeout,
                };
            let duration = start.elapsed();
            (
                idx,
                ToolResult {
                    tool_use_id: call_id,
                    tool_name: call_name,
                    outcome: outcome.clone(),
                },
                ToolCallTrace {
                    tool_name: String::new(), // filled below
                    duration,
                    outcome,
                },
            )
        });
    }

    // Collect results, reorder by original index.
    let mut indexed_results: Vec<(usize, ToolResult, ToolCallTrace)> = Vec::with_capacity(calls_to_run.len());
    while let Some(result) = set.join_next().await {
        match result {
            Ok(tuple) => indexed_results.push(tuple),
            Err(e) => {
                tracing::error!("tool execution task panicked: {e}");
            }
        }
    }
    indexed_results.sort_by_key(|(idx, _, _)| *idx);

    indexed_results
        .into_iter()
        .map(|(_, mut result, _)| {
            result
        })
        .collect()
}

async fn execute_single(
    registry: &ToolRegistry,
    tool_name: &str,
    input: Value,
) -> Result<Value, String> {
    let tool = registry
        .get(tool_name)
        .ok_or_else(|| format!("tool '{}' not found in registry", tool_name))?;
    tool.execute(input).await
}

/// Check if two sets of tool calls are duplicates (same name+input pairs, ignoring IDs).
pub fn is_duplicate(a: &[ToolCall], b: &[ToolCall]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    // Sort by name for comparison, then compare inputs.
    let mut a_sorted: Vec<(&str, &Value)> = a.iter().map(|c| (c.name.as_str(), &c.input)).collect();
    let mut b_sorted: Vec<(&str, &Value)> = b.iter().map(|c| (c.name.as_str(), &c.input)).collect();
    a_sorted.sort_by_key(|(name, _)| *name);
    b_sorted.sort_by_key(|(name, _)| *name);
    a_sorted
        .iter()
        .zip(b_sorted.iter())
        .all(|((na, ia), (nb, ib))| na == nb && ia == ib)
}
```

- [ ] **Step 5: Update mod.rs**

In `crates/proxy/src/tools/mod.rs`:

```rust
// Tool execution engine and registry

pub mod builtin;
pub mod execution;
pub mod policy;
pub mod registry;
pub mod trace;

pub use execution::{LoopConfig, ToolCall, ToolResult};
pub use policy::{PolicyAction, PolicyRule, ToolExecutionPolicy};
pub use registry::{Tool, ToolRegistry};
pub use trace::{LoopTrace, TerminationReason, ToolCallTrace, ToolOutcome};
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p anyllm_proxy execution::tests -- -v`
Expected: 6 tests PASS

- [ ] **Step 7: Commit**

```bash
git add crates/proxy/src/tools/execution.rs crates/proxy/src/tools/registry.rs crates/proxy/src/tools/mod.rs
git commit -m "feat(tools): add ToolExecutionEngine core with partition and parallel execution"
```

---

### Task 4: Config Parsing for Tool Execution

**Files:**
- Modify: `crates/proxy/src/config/simple.rs`

- [ ] **Step 1: Write the failing test**

Add to the test module in `crates/proxy/src/config/simple.rs`:

```rust
#[test]
fn parse_tool_execution_config() {
    let yaml = r#"
models:
  - gpt-4o

tool_execution:
  max_iterations: 3
  tool_timeout_secs: 60
  total_timeout_secs: 600

builtin_tools:
  execute_bash:
    enabled: false
  read_file:
    enabled: true
    policy: allow

mcp_servers:
  - name: github
    url: https://mcp.github.com/sse
    policy: allow
"#;
    let config: SimpleConfig = serde_yaml::from_str(yaml).unwrap();
    let te = config.tool_execution.unwrap();
    assert_eq!(te.max_iterations, Some(3));
    assert_eq!(te.tool_timeout_secs, Some(60));
    assert_eq!(te.total_timeout_secs, Some(600));

    let builtins = config.builtin_tools.unwrap();
    let bash = builtins.get("execute_bash").unwrap();
    assert!(!bash.enabled);
    let rf = builtins.get("read_file").unwrap();
    assert!(rf.enabled);
    assert_eq!(rf.policy.as_deref(), Some("allow"));

    let mcp = config.mcp_servers.unwrap();
    assert_eq!(mcp.len(), 1);
    assert_eq!(mcp[0].name, "github");
    assert_eq!(mcp[0].policy.as_deref(), Some("allow"));
}

#[test]
fn parse_config_without_tool_sections() {
    let yaml = r#"
models:
  - gpt-4o
"#;
    let config: SimpleConfig = serde_yaml::from_str(yaml).unwrap();
    assert!(config.tool_execution.is_none());
    assert!(config.builtin_tools.is_none());
    assert!(config.mcp_servers.is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p anyllm_proxy parse_tool_execution_config -- -v`
Expected: FAIL (fields do not exist on SimpleConfig)

- [ ] **Step 3: Write the implementation**

Add to `crates/proxy/src/config/simple.rs`, the new config structs after `SimpleModelFull`:

```rust
/// Tool execution loop configuration.
#[derive(Debug, Deserialize)]
pub struct ToolExecutionConfig {
    #[serde(default)]
    pub max_iterations: Option<usize>,
    #[serde(default)]
    pub tool_timeout_secs: Option<u64>,
    #[serde(default)]
    pub total_timeout_secs: Option<u64>,
    #[serde(default)]
    pub max_tool_calls_per_turn: Option<usize>,
}

/// Configuration for a single builtin tool.
#[derive(Debug, Deserialize)]
pub struct BuiltinToolConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub policy: Option<String>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

fn default_true() -> bool {
    true
}

/// MCP server configuration entry.
#[derive(Debug, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub policy: Option<String>,
}
```

Add to `SimpleConfig` struct:

```rust
    /// Tool execution loop settings.
    #[serde(default)]
    pub tool_execution: Option<ToolExecutionConfig>,
    /// Builtin tool overrides keyed by tool name.
    #[serde(default)]
    pub builtin_tools: Option<HashMap<String, BuiltinToolConfig>>,
    /// MCP server definitions.
    #[serde(default)]
    pub mcp_servers: Option<Vec<McpServerConfig>>,
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p anyllm_proxy parse_tool_execution_config -- -v && cargo test -p anyllm_proxy parse_config_without_tool_sections -- -v`
Expected: 2 tests PASS

- [ ] **Step 5: Run full test suite to check for regressions**

Run: `cargo test -p anyllm_proxy`
Expected: All existing tests PASS

- [ ] **Step 6: Commit**

```bash
git add crates/proxy/src/config/simple.rs
git commit -m "feat(config): add tool_execution, builtin_tools, and mcp_servers config sections"
```

---

### Task 5: Config-to-Policy Conversion

**Files:**
- Modify: `crates/proxy/src/config/simple.rs` (add conversion function)

- [ ] **Step 1: Write the failing test**

Add to the test module in `crates/proxy/src/config/simple.rs`:

```rust
#[test]
fn build_tool_policy_from_config() {
    let yaml = r#"
models:
  - gpt-4o

builtin_tools:
  execute_bash:
    enabled: true
    policy: deny
  read_file:
    enabled: true
    policy: allow
    timeout_secs: 10

mcp_servers:
  - name: github
    url: https://mcp.github.com/sse
    policy: allow
"#;
    let config: SimpleConfig = serde_yaml::from_str(yaml).unwrap();
    let (policy, loop_config) = config.build_tool_config();

    use crate::tools::policy::PolicyAction;
    assert_eq!(policy.resolve("execute_bash"), PolicyAction::Deny);
    assert_eq!(policy.resolve("read_file"), PolicyAction::Allow);
    assert_eq!(policy.resolve("unknown"), PolicyAction::PassThrough);

    // Check timeout override for read_file
    let rule = policy.find_rule("read_file").unwrap();
    assert_eq!(rule.timeout, Some(std::time::Duration::from_secs(10)));

    // MCP tools from github server get prefixed: "mcp_github_*"
    // Policy should allow them via glob rule
    assert_eq!(policy.resolve("mcp_github_search_repos"), PolicyAction::Allow);

    // Default loop config when tool_execution section is absent
    assert_eq!(loop_config.max_iterations, 1);
}

#[test]
fn build_tool_policy_with_loop_config() {
    let yaml = r#"
models:
  - gpt-4o

tool_execution:
  max_iterations: 5
  tool_timeout_secs: 45
"#;
    let config: SimpleConfig = serde_yaml::from_str(yaml).unwrap();
    let (_policy, loop_config) = config.build_tool_config();
    assert_eq!(loop_config.max_iterations, 5);
    assert_eq!(loop_config.tool_timeout, std::time::Duration::from_secs(45));
    // total_timeout uses default when not specified
    assert_eq!(loop_config.total_timeout, std::time::Duration::from_secs(300));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p anyllm_proxy build_tool_policy -- -v`
Expected: FAIL (method does not exist)

- [ ] **Step 3: Write the implementation**

Add to `crates/proxy/src/config/simple.rs`:

```rust
impl SimpleConfig {
    /// Build a ToolExecutionPolicy and LoopConfig from this config.
    pub fn build_tool_config(&self) -> (crate::tools::ToolExecutionPolicy, crate::tools::LoopConfig) {
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
                rules.push(PolicyRule {
                    tool_name: name.clone(),
                    action,
                    timeout: cfg.timeout_secs.map(std::time::Duration::from_secs),
                    max_concurrency: None,
                });
            }
        }

        // MCP server rules: create a glob rule for each server's tools.
        if let Some(ref servers) = self.mcp_servers {
            for server in servers {
                let action = match server.policy.as_deref() {
                    Some("allow") => PolicyAction::Allow,
                    Some("deny") => PolicyAction::Deny,
                    _ => PolicyAction::PassThrough,
                };
                // MCP tools are prefixed: mcp_{server_name}_*
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p anyllm_proxy build_tool_policy -- -v`
Expected: 2 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/proxy/src/config/simple.rs
git commit -m "feat(config): add build_tool_config() to convert YAML to policy + loop config"
```

---

### Task 6: MCP Server Manager

**Files:**
- Create: `crates/proxy/src/tools/mcp.rs`
- Modify: `crates/proxy/src/tools/mod.rs`

- [ ] **Step 1: Write the failing test**

In `crates/proxy/src/tools/mcp.rs`:

```rust
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
                name: "search_repos".to_string(),
                description: "Search GitHub repos".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
            },
            McpToolDef {
                name: "create_issue".to_string(),
                description: "Create a GitHub issue".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
            },
        ];
        mgr.register_server_blocking(
            "github",
            "https://mcp.github.com/sse",
            tools,
        );

        let servers = mgr.list_servers_blocking();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "github");
        assert_eq!(servers[0].tools.len(), 2);

        // Tool names are prefixed with mcp_{server_name}_
        assert_eq!(
            mgr.find_server_for_tool_blocking("mcp_github_search_repos"),
            Some("github".to_string())
        );
        assert_eq!(
            mgr.find_server_for_tool_blocking("mcp_github_create_issue"),
            Some("github".to_string())
        );
        assert!(mgr.find_server_for_tool_blocking("mcp_slack_send").is_none());
    }

    #[test]
    fn remove_server_cleans_up_tool_mappings() {
        let mgr = McpServerManager::new();
        mgr.register_server_blocking(
            "github",
            "https://mcp.github.com/sse",
            vec![McpToolDef {
                name: "search".to_string(),
                description: "search".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
            }],
        );
        assert!(mgr.find_server_for_tool_blocking("mcp_github_search").is_some());

        mgr.remove_server_blocking("github");
        assert!(mgr.list_servers_blocking().is_empty());
        assert!(mgr.find_server_for_tool_blocking("mcp_github_search").is_none());
    }

    #[test]
    fn mcp_tool_name_prefixing() {
        assert_eq!(mcp_tool_name("github", "search"), "mcp_github_search");
        assert_eq!(mcp_tool_name("my-server", "do_thing"), "mcp_my-server_do_thing");
    }

    #[test]
    fn as_anthropic_tools_returns_prefixed_names() {
        let mgr = McpServerManager::new();
        mgr.register_server_blocking(
            "github",
            "https://example.com/sse",
            vec![McpToolDef {
                name: "search".to_string(),
                description: "Search".to_string(),
                input_schema: serde_json::json!({"type": "object", "properties": {}}),
            }],
        );
        let tools = mgr.as_anthropic_tools_blocking();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "mcp_github_search");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p anyllm_proxy mcp::tests -- -v`
Expected: FAIL (module does not exist)

- [ ] **Step 3: Write the implementation**

In `crates/proxy/src/tools/mcp.rs`:

```rust
use std::collections::HashMap;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    /// Maps prefixed tool name -> server name.
    tool_to_server: RwLock<HashMap<String, String>>,
}

impl McpServerManager {
    pub fn new() -> Self {
        Self {
            servers: RwLock::new(HashMap::new()),
            tool_to_server: RwLock::new(HashMap::new()),
        }
    }

    /// Register a server with its discovered tools. Replaces any existing entry.
    pub fn register_server_blocking(
        &self,
        name: &str,
        url: &str,
        tools: Vec<McpToolDef>,
    ) {
        // Remove old mappings if server already exists.
        self.remove_server_blocking(name);

        let mut tool_map = self.tool_to_server.write().unwrap();
        for tool in &tools {
            let prefixed = mcp_tool_name(name, &tool.name);
            tool_map.insert(prefixed, name.to_string());
        }

        let server = McpServer {
            name: name.to_string(),
            url: url.to_string(),
            tools,
        };
        self.servers.write().unwrap().insert(name.to_string(), server);
    }

    /// Remove a server and all its tool mappings.
    pub fn remove_server_blocking(&self, name: &str) {
        if let Some(server) = self.servers.write().unwrap().remove(name) {
            let mut tool_map = self.tool_to_server.write().unwrap();
            for tool in &server.tools {
                let prefixed = mcp_tool_name(name, &tool.name);
                tool_map.remove(&prefixed);
            }
        }
    }

    /// List all registered servers.
    pub fn list_servers_blocking(&self) -> Vec<McpServer> {
        self.servers.read().unwrap().values().cloned().collect()
    }

    /// Find which server owns a prefixed tool name.
    pub fn find_server_for_tool_blocking(&self, prefixed_name: &str) -> Option<String> {
        self.tool_to_server
            .read()
            .unwrap()
            .get(prefixed_name)
            .cloned()
    }

    /// Get all MCP tools as Anthropic tool definitions (with prefixed names).
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

    /// Call an MCP tool via SSE JSON-RPC. Returns the result value or error.
    pub async fn call_tool(
        &self,
        prefixed_name: &str,
        input: Value,
    ) -> Result<Value, String> {
        let (server_name, original_name) = parse_mcp_tool_name(prefixed_name)
            .ok_or_else(|| format!("invalid MCP tool name: {}", prefixed_name))?;

        let server_url = {
            let servers = self.servers.read().unwrap();
            let server = servers
                .get(server_name)
                .ok_or_else(|| format!("MCP server '{}' not found", server_name))?;
            server.url.clone()
        };

        // Send JSON-RPC tools/call request via HTTP POST.
        let client = reqwest::Client::new();
        let rpc_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": original_name,
                "arguments": input,
            }
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

        // JSON-RPC response: check for error field.
        if let Some(error) = body.get("error") {
            let msg = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown MCP error");
            return Err(format!("MCP tool error: {}", msg));
        }

        // Extract result.
        body.get("result")
            .cloned()
            .ok_or_else(|| "MCP response missing 'result' field".to_string())
    }

    /// Discover tools from an MCP server by calling tools/list.
    pub async fn discover_tools(url: &str) -> Result<Vec<McpToolDef>, String> {
        let client = reqwest::Client::new();
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

        let tools: Vec<McpToolDef> = serde_json::from_value(tools_value.clone())
            .map_err(|e| format!("MCP tools parse error: {}", e))?;

        Ok(tools)
    }
}

impl Default for McpServerManager {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 4: Update mod.rs**

In `crates/proxy/src/tools/mod.rs`:

```rust
// Tool execution engine and registry

pub mod builtin;
pub mod execution;
pub mod mcp;
pub mod policy;
pub mod registry;
pub mod trace;

pub use execution::{LoopConfig, ToolCall, ToolResult};
pub use mcp::McpServerManager;
pub use policy::{PolicyAction, PolicyRule, ToolExecutionPolicy};
pub use registry::{Tool, ToolRegistry};
pub use trace::{LoopTrace, TerminationReason, ToolCallTrace, ToolOutcome};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p anyllm_proxy mcp::tests -- -v`
Expected: 5 tests PASS

- [ ] **Step 6: Commit**

```bash
git add crates/proxy/src/tools/mcp.rs crates/proxy/src/tools/mod.rs
git commit -m "feat(tools): add McpServerManager with tool discovery and JSON-RPC execution"
```

---

### Task 7: MCP Tool Adapter (Bridge MCP to Tool Trait)

**Files:**
- Modify: `crates/proxy/src/tools/mcp.rs` (add `McpToolAdapter`)

- [ ] **Step 1: Write the failing test**

Add to the test module in `crates/proxy/src/tools/mcp.rs`:

```rust
    #[test]
    fn mcp_tool_adapter_implements_tool_trait() {
        let mgr = Arc::new(McpServerManager::new());
        let adapter = McpToolAdapter {
            prefixed_name: "mcp_github_search".to_string(),
            description: "Search repos".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
            manager: mgr,
        };

        // Verify it satisfies the Tool trait.
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
        );

        let mut registry = crate::tools::ToolRegistry::new();
        register_mcp_tools(&mgr, &mut registry);

        assert!(registry.contains("mcp_github_search"));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p anyllm_proxy mcp::tests::mcp_tool_adapter -- -v`
Expected: FAIL (`McpToolAdapter` does not exist)

- [ ] **Step 3: Write the implementation**

Add to `crates/proxy/src/tools/mcp.rs`:

```rust
use std::sync::Arc;

/// Adapter that wraps an MCP tool as a `Tool` trait implementor.
/// Delegates execution to the `McpServerManager::call_tool()`.
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
pub fn register_mcp_tools(manager: &Arc<McpServerManager>, registry: &mut crate::tools::ToolRegistry) {
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p anyllm_proxy mcp::tests -- -v`
Expected: 7 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/proxy/src/tools/mcp.rs
git commit -m "feat(tools): add McpToolAdapter bridging MCP tools to Tool trait"
```

---

### Task 8: MCP Admin Endpoints

**Files:**
- Modify: `crates/proxy/src/admin/routes.rs`
- Modify: `crates/proxy/src/admin/state.rs` (add `McpServerManager` to `SharedState`)

- [ ] **Step 1: Write the failing test**

Create a new test in `crates/proxy/tests/mcp_admin.rs`:

```rust
//! Integration tests for MCP server admin endpoints.
//! These test the HTTP endpoints directly.

// NOTE: This test validates the admin endpoint JSON contract.
// Actual MCP server connectivity is not tested here (requires a real MCP server).

use serde_json::json;

#[test]
fn mcp_admin_list_empty() {
    // When no MCP servers are configured, GET returns an empty list.
    let mgr = anyllm_proxy::tools::McpServerManager::new();
    let servers = mgr.list_servers_blocking();
    assert!(servers.is_empty());
}

#[test]
fn mcp_admin_register_and_list() {
    use anyllm_proxy::tools::mcp::{McpServerManager, McpToolDef};

    let mgr = McpServerManager::new();
    mgr.register_server_blocking(
        "test-server",
        "http://localhost:9999/sse",
        vec![McpToolDef {
            name: "ping".to_string(),
            description: "Ping".to_string(),
            input_schema: json!({"type": "object"}),
        }],
    );

    let servers = mgr.list_servers_blocking();
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].name, "test-server");
    assert_eq!(servers[0].tools.len(), 1);
}

#[test]
fn mcp_admin_remove() {
    use anyllm_proxy::tools::mcp::{McpServerManager, McpToolDef};

    let mgr = McpServerManager::new();
    mgr.register_server_blocking(
        "removable",
        "http://localhost:9999/sse",
        vec![McpToolDef {
            name: "tool1".to_string(),
            description: "Tool 1".to_string(),
            input_schema: json!({"type": "object"}),
        }],
    );
    assert_eq!(mgr.list_servers_blocking().len(), 1);

    mgr.remove_server_blocking("removable");
    assert!(mgr.list_servers_blocking().is_empty());
    assert!(mgr.find_server_for_tool_blocking("mcp_removable_tool1").is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test mcp_admin -- -v`
Expected: FAIL (test file does not exist yet, or compilation fails)

- [ ] **Step 3: Add McpServerManager to SharedState**

In `crates/proxy/src/admin/state.rs`, add to the `SharedState` struct:

```rust
    pub mcp_manager: Option<Arc<crate::tools::McpServerManager>>,
```

Update the `SharedState` construction in `crates/proxy/src/main.rs` to include:

```rust
    mcp_manager: None, // Populated later if MCP servers are configured.
```

- [ ] **Step 4: Add admin route handlers**

In `crates/proxy/src/admin/routes.rs`, add the MCP server endpoints:

```rust
/// GET /admin/api/mcp-servers - List all MCP servers and their tools.
async fn list_mcp_servers(
    State(shared): State<SharedState>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    use axum::http::StatusCode;

    let Some(ref mgr) = shared.mcp_manager else {
        return (StatusCode::OK, axum::Json(serde_json::json!({"servers": []}))).into_response();
    };
    let servers = mgr.list_servers_blocking();
    (StatusCode::OK, axum::Json(serde_json::json!({"servers": servers}))).into_response()
}

/// POST /admin/api/mcp-servers - Add an MCP server. Body: { name, url }.
/// Attempts tool discovery and registers the server.
async fn add_mcp_server(
    State(shared): State<SharedState>,
    axum::Json(body): axum::Json<serde_json::Value>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    use axum::http::StatusCode;

    let Some(ref mgr) = shared.mcp_manager else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({"error": "MCP support not enabled"})),
        ).into_response();
    };

    let name = match body.get("name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({"error": "missing 'name' field"})),
            ).into_response();
        }
    };
    let url = match body.get("url").and_then(|v| v.as_str()) {
        Some(u) => u.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({"error": "missing 'url' field"})),
            ).into_response();
        }
    };

    // Discover tools from the MCP server.
    match crate::tools::mcp::McpServerManager::discover_tools(&url).await {
        Ok(tools) => {
            let tool_count = tools.len();
            mgr.register_server_blocking(&name, &url, tools);
            tracing::info!(server = %name, tools = tool_count, "MCP server registered");
            (
                StatusCode::CREATED,
                axum::Json(serde_json::json!({
                    "name": name,
                    "url": url,
                    "tools_discovered": tool_count,
                })),
            ).into_response()
        }
        Err(e) => {
            tracing::warn!(server = %name, error = %e, "MCP tool discovery failed");
            (
                StatusCode::BAD_GATEWAY,
                axum::Json(serde_json::json!({"error": e})),
            ).into_response()
        }
    }
}

/// DELETE /admin/api/mcp-servers/:name - Remove an MCP server.
async fn remove_mcp_server(
    State(shared): State<SharedState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    use axum::http::StatusCode;

    let Some(ref mgr) = shared.mcp_manager else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({"error": "MCP support not enabled"})),
        ).into_response();
    };

    mgr.remove_server_blocking(&name);
    tracing::info!(server = %name, "MCP server removed");
    (StatusCode::OK, axum::Json(serde_json::json!({"removed": name}))).into_response()
}
```

Add routes to `admin_router()` in the protected section:

```rust
        .route("/admin/api/mcp-servers", get(list_mcp_servers).post(add_mcp_server))
        .route("/admin/api/mcp-servers/:name", delete(remove_mcp_server))
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test mcp_admin -- -v && cargo test -p anyllm_proxy -- -v`
Expected: All tests PASS

- [ ] **Step 6: Commit**

```bash
git add crates/proxy/src/admin/routes.rs crates/proxy/src/admin/state.rs crates/proxy/src/main.rs crates/proxy/tests/mcp_admin.rs
git commit -m "feat(admin): add MCP server management endpoints (list, add, remove)"
```

---

### Task 9: Wire ToolExecutionEngine into AppState

**Files:**
- Modify: `crates/proxy/src/server/routes.rs` (add `tool_engine` to `AppState`)
- Modify: `crates/proxy/src/main.rs` (build engine from config, inject into state)

- [ ] **Step 1: Add tool_engine field to AppState**

In `crates/proxy/src/server/routes.rs`, add to the `AppState` struct:

```rust
    pub tool_engine: Option<Arc<ToolEngineState>>,
```

And define `ToolEngineState`:

```rust
/// Shared state for tool execution, stored in AppState.
pub struct ToolEngineState {
    pub registry: Arc<crate::tools::ToolRegistry>,
    pub policy: Arc<crate::tools::ToolExecutionPolicy>,
    pub loop_config: crate::tools::LoopConfig,
    pub mcp_manager: Option<Arc<crate::tools::McpServerManager>>,
}
```

- [ ] **Step 2: Build tool engine in main.rs**

In `crates/proxy/src/main.rs`, after loading `SimpleConfig`, add engine construction:

```rust
    // Build tool execution engine from config.
    let tool_engine_state = {
        // If SimpleConfig was loaded, extract tool config from it.
        // Otherwise use defaults.
        let (policy, loop_config) = if let Some(ref simple_cfg) = simple_config {
            simple_cfg.build_tool_config()
        } else {
            (
                crate::tools::ToolExecutionPolicy::default(),
                crate::tools::LoopConfig::default(),
            )
        };

        let mut registry = crate::tools::ToolRegistry::new();

        // Register builtins based on config.
        let builtins_config = simple_config.as_ref().and_then(|c| c.builtin_tools.as_ref());
        let bash_enabled = builtins_config
            .and_then(|b| b.get("execute_bash"))
            .map(|c| c.enabled)
            .unwrap_or(true);
        let read_file_enabled = builtins_config
            .and_then(|b| b.get("read_file"))
            .map(|c| c.enabled)
            .unwrap_or(true);

        if bash_enabled {
            registry.register(Box::new(crate::tools::builtin::bash::BashTool));
        }
        if read_file_enabled {
            registry.register(Box::new(crate::tools::builtin::read_file::ReadFileTool));
        }

        // MCP servers from config (discovery happens at startup).
        let mcp_manager = Arc::new(crate::tools::McpServerManager::new());
        if let Some(ref simple_cfg) = simple_config {
            if let Some(ref mcp_servers) = simple_cfg.mcp_servers {
                for mcp_cfg in mcp_servers {
                    match crate::tools::mcp::McpServerManager::discover_tools(&mcp_cfg.url).await {
                        Ok(tools) => {
                            let count = tools.len();
                            mcp_manager.register_server_blocking(&mcp_cfg.name, &mcp_cfg.url, tools);
                            tracing::info!(
                                server = %mcp_cfg.name,
                                tools = count,
                                "MCP server registered from config"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                server = %mcp_cfg.name,
                                error = %e,
                                "MCP server discovery failed at startup, skipping"
                            );
                        }
                    }
                }
                // Register MCP tools into the registry.
                crate::tools::mcp::register_mcp_tools(&mcp_manager, &mut registry);
            }
        }

        Arc::new(routes::ToolEngineState {
            registry: Arc::new(registry),
            policy: Arc::new(policy),
            loop_config,
            mcp_manager: Some(mcp_manager),
        })
    };
```

Pass `Some(tool_engine_state)` when constructing each `AppState`.

- [ ] **Step 3: Verify compilation**

Run: `cargo build -p anyllm_proxy`
Expected: Compiles without errors

- [ ] **Step 4: Run full test suite**

Run: `cargo test -p anyllm_proxy`
Expected: All tests PASS (no behavior change yet, just wiring)

- [ ] **Step 5: Commit**

```bash
git add crates/proxy/src/server/routes.rs crates/proxy/src/main.rs
git commit -m "feat(tools): wire ToolEngineState into AppState with config-driven setup"
```

---

### Task 10: Non-Streaming Tool Execution in Chat Completions Handler

**Files:**
- Modify: `crates/proxy/src/server/chat_completions.rs`
- Modify: `crates/proxy/src/tools/execution.rs` (add `run_non_streaming()`)

This is the integration point where tool execution actually happens. The handler calls the backend, checks the response for tool_use blocks, and if any are auto-executable, runs them and makes a follow-up backend call.

- [ ] **Step 1: Write the `run_non_streaming` function**

In `crates/proxy/src/tools/execution.rs`, add:

```rust
/// Result of the non-streaming tool execution loop.
pub struct EngineResult {
    /// The final LLM response (after tool execution, if any).
    pub response: anyllm_translate::anthropic::MessageResponse,
    /// Observability trace of the execution loop.
    pub trace: LoopTrace,
    /// Whether any tools were executed (for logging).
    pub tools_executed: bool,
}

/// Extract ToolCall structs from an Anthropic MessageResponse.
pub fn extract_tool_calls(response: &anyllm_translate::anthropic::MessageResponse) -> Vec<ToolCall> {
    response
        .content
        .iter()
        .filter_map(|block| {
            if let anyllm_translate::anthropic::ContentBlock::ToolUse { id, name, input } = block {
                Some(ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                })
            } else {
                None
            }
        })
        .collect()
}

/// Convert tool execution results to Anthropic ToolResult content blocks
/// wrapped in a user message, for appending to the conversation.
pub fn tool_results_to_user_message(
    results: &[ToolResult],
) -> anyllm_translate::anthropic::InputMessage {
    let blocks: Vec<anyllm_translate::anthropic::ContentBlock> = results
        .iter()
        .map(|r| {
            let (content_text, is_error) = match &r.outcome {
                ToolOutcome::Success(v) => {
                    (serde_json::to_string(v).unwrap_or_default(), false)
                }
                ToolOutcome::Error { message, .. } => (message.clone(), true),
                ToolOutcome::Timeout => {
                    ("Tool execution timed out".to_string(), true)
                }
            };
            anyllm_translate::anthropic::ContentBlock::ToolResult {
                tool_use_id: r.tool_use_id.clone(),
                content: Some(anyllm_translate::anthropic::ToolResultContent::Text(content_text)),
                is_error: Some(is_error),
            }
        })
        .collect();

    anyllm_translate::anthropic::InputMessage {
        role: anyllm_translate::anthropic::Role::User,
        content: anyllm_translate::anthropic::Content::Blocks(blocks),
    }
}

/// Convert an Anthropic MessageResponse's assistant content into an InputMessage
/// for appending to the conversation history.
pub fn response_to_assistant_message(
    response: &anyllm_translate::anthropic::MessageResponse,
) -> anyllm_translate::anthropic::InputMessage {
    anyllm_translate::anthropic::InputMessage {
        role: anyllm_translate::anthropic::Role::Assistant,
        content: anyllm_translate::anthropic::Content::Blocks(response.content.clone()),
    }
}
```

- [ ] **Step 2: Write the failing test for extract_tool_calls**

Add to the test module in `execution.rs`:

```rust
    #[test]
    fn extract_tool_calls_from_response() {
        use anyllm_translate::anthropic::{ContentBlock, MessageResponse, StopReason, Usage};

        let response = MessageResponse {
            id: "msg_1".to_string(),
            model: "test".to_string(),
            role: "assistant".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: "Let me check that.".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "tu_1".to_string(),
                    name: "echo".to_string(),
                    input: serde_json::json!({"text": "hello"}),
                },
            ],
            stop_reason: Some(StopReason::ToolUse),
            stop_sequence: None,
            usage: Usage {
                input_tokens: 10,
                output_tokens: 20,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };

        let calls = extract_tool_calls(&response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "echo");
        assert_eq!(calls[0].id, "tu_1");
    }

    #[test]
    fn extract_tool_calls_no_tools() {
        use anyllm_translate::anthropic::{ContentBlock, MessageResponse, StopReason, Usage};

        let response = MessageResponse {
            id: "msg_2".to_string(),
            model: "test".to_string(),
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: "Just text.".to_string(),
            }],
            stop_reason: Some(StopReason::EndTurn),
            stop_sequence: None,
            usage: Usage {
                input_tokens: 5,
                output_tokens: 10,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };

        let calls = extract_tool_calls(&response);
        assert!(calls.is_empty());
    }
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p anyllm_proxy extract_tool_calls -- -v`
Expected: FAIL (function does not exist yet, or compilation error from types)

- [ ] **Step 4: Verify tests pass after implementation**

Run: `cargo test -p anyllm_proxy execution::tests -- -v`
Expected: All tests PASS

- [ ] **Step 5: Integrate into chat_completions handler**

In `crates/proxy/src/server/chat_completions.rs`, in the non-streaming path, after receiving the backend response and translating it to Anthropic format, add:

```rust
    // --- Tool execution check ---
    if let Some(ref engine) = state.tool_engine {
        let tool_calls = crate::tools::execution::extract_tool_calls(&anthropic_response);
        if !tool_calls.is_empty() {
            let (auto_exec, _pass_through) = crate::tools::execution::partition_tool_calls(
                &tool_calls,
                &engine.registry,
                &engine.policy,
            );

            if !auto_exec.is_empty() {
                // Execute auto-execute tools in parallel.
                let results = crate::tools::execution::execute_tool_calls(
                    &auto_exec,
                    &engine.registry,
                    &engine.policy,
                    &engine.loop_config,
                )
                .await;

                // Build follow-up messages: original + assistant + tool results.
                let mut follow_up_messages = anthropic_req.messages.clone();
                follow_up_messages.push(
                    crate::tools::execution::response_to_assistant_message(&anthropic_response),
                );
                follow_up_messages.push(
                    crate::tools::execution::tool_results_to_user_message(&results),
                );

                // Make follow-up backend call.
                let mut follow_up_req = anthropic_req.clone();
                follow_up_req.messages = follow_up_messages;

                // Translate and call backend again.
                // (Use the same translation path as the original request.)
                // The follow-up response replaces the original.
                // ... (call backend with follow_up_req, translate response)

                tracing::info!(
                    tools_executed = results.len(),
                    "tool execution loop completed"
                );
            }
        }
    }
```

**Note to implementer:** The exact integration depends on the handler's structure. The pattern is:
1. After receiving and translating the first response, check for tool calls.
2. If auto-executable, execute them.
3. Build follow-up request with tool results appended.
4. Call backend again with the same translation path.
5. Return the follow-up response.

The handler-specific details (which backend client method to call, how to translate) should follow the existing patterns in the handler.

- [ ] **Step 6: Run full test suite**

Run: `cargo test -p anyllm_proxy`
Expected: All tests PASS

- [ ] **Step 7: Commit**

```bash
git add crates/proxy/src/tools/execution.rs crates/proxy/src/server/chat_completions.rs
git commit -m "feat(tools): integrate non-streaming tool execution into chat completions handler"
```

---

### Task 11: Non-Streaming Tool Execution in Messages Handler

**Files:**
- Modify: `crates/proxy/src/server/mod.rs` (Anthropic messages handler)

- [ ] **Step 1: Add tool execution to the messages handler**

The messages handler (`/v1/messages`) follows the same pattern as chat completions. After translating the Anthropic request to OpenAI, calling the backend, and translating back:

```rust
    // --- Tool execution check (same as chat_completions) ---
    if let Some(ref engine) = state.tool_engine {
        let tool_calls = crate::tools::execution::extract_tool_calls(&anthropic_response);
        if !tool_calls.is_empty() {
            let (auto_exec, _pass_through) = crate::tools::execution::partition_tool_calls(
                &tool_calls,
                &engine.registry,
                &engine.policy,
            );

            if !auto_exec.is_empty() {
                let results = crate::tools::execution::execute_tool_calls(
                    &auto_exec,
                    &engine.registry,
                    &engine.policy,
                    &engine.loop_config,
                )
                .await;

                let mut follow_up_messages = anthropic_req.messages.clone();
                follow_up_messages.push(
                    crate::tools::execution::response_to_assistant_message(&anthropic_response),
                );
                follow_up_messages.push(
                    crate::tools::execution::tool_results_to_user_message(&results),
                );

                let mut follow_up_req = anthropic_req.clone();
                follow_up_req.messages = follow_up_messages;

                // Re-translate and call backend.
                // Return the follow-up response.
            }
        }
    }
```

- [ ] **Step 2: Run full test suite**

Run: `cargo test -p anyllm_proxy`
Expected: All tests PASS

- [ ] **Step 3: Commit**

```bash
git add crates/proxy/src/server/mod.rs
git commit -m "feat(tools): integrate non-streaming tool execution into messages handler"
```

---

### Task 12: Streaming Tool Execution (Collect-Then-Execute)

**Files:**
- Modify: `crates/proxy/src/server/chat_completions.rs` (streaming path)
- Modify: `crates/proxy/src/server/streaming.rs` (if needed for shared logic)

For phase C streaming: collect the full streamed response, check for tool calls, execute, then stream the follow-up response.

- [ ] **Step 1: Add streaming tool execution**

In the streaming handler, after the stream completes and the full response is assembled:

```rust
    // After collecting all stream chunks and building the assembled response:
    if let Some(ref engine) = state.tool_engine {
        let tool_calls = crate::tools::execution::extract_tool_calls(&assembled_response);
        let (auto_exec, _pass_through) = crate::tools::execution::partition_tool_calls(
            &tool_calls,
            &engine.registry,
            &engine.policy,
        );

        if !auto_exec.is_empty() {
            // Execute tools.
            let results = crate::tools::execution::execute_tool_calls(
                &auto_exec,
                &engine.registry,
                &engine.policy,
                &engine.loop_config,
            )
            .await;

            // Build follow-up request.
            let mut follow_up_messages = original_messages.clone();
            follow_up_messages.push(
                crate::tools::execution::response_to_assistant_message(&assembled_response),
            );
            follow_up_messages.push(
                crate::tools::execution::tool_results_to_user_message(&results),
            );

            // Make follow-up streaming backend call.
            // Stream the follow-up response to the client.
            // The client sees: initial stream -> pause (tool execution) -> follow-up stream.
        }
    }
```

**Note to implementer:** The streaming path is more complex. The key insight is:
1. Stream initial response chunks to the client as they arrive.
2. When the stream ends, check if the assembled response has auto-executable tool calls.
3. If yes, execute tools, then start a new streaming backend call.
4. Stream the follow-up response chunks to the same client channel.
5. The client sees one continuous stream with a pause during tool execution.

- [ ] **Step 2: Run full test suite**

Run: `cargo test -p anyllm_proxy`
Expected: All tests PASS

- [ ] **Step 3: Commit**

```bash
git add crates/proxy/src/server/chat_completions.rs crates/proxy/src/server/streaming.rs
git commit -m "feat(tools): add collect-then-execute streaming tool execution"
```

---

### Task 13: Integration Test

**Files:**
- Create: `crates/proxy/tests/tool_execution.rs`

- [ ] **Step 1: Write integration test**

```rust
//! Integration tests for the tool execution engine.
//! Tests the core execution flow without a running proxy.

use std::sync::Arc;

use anyllm_proxy::tools::execution::{
    execute_tool_calls, extract_tool_calls, is_duplicate, partition_tool_calls,
    tool_results_to_user_message, LoopConfig, ToolCall,
};
use anyllm_proxy::tools::policy::{PolicyAction, PolicyRule, ToolExecutionPolicy};
use anyllm_proxy::tools::registry::ToolRegistry;
use anyllm_proxy::tools::trace::ToolOutcome;

/// Test tool that returns input uppercase.
struct UpperTool;

impl anyllm_proxy::tools::Tool for UpperTool {
    fn name(&self) -> &str {
        "upper"
    }
    fn description(&self) -> &str {
        "Uppercases text"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {"text": {"type": "string"}},
            "required": ["text"]
        })
    }
    fn execute<'a>(
        &'a self,
        input: serde_json::Value,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<serde_json::Value, String>> + Send + 'a>>
    {
        Box::pin(async move {
            let text = input["text"].as_str().unwrap_or("").to_uppercase();
            Ok(serde_json::json!({"result": text}))
        })
    }
}

fn setup() -> (Arc<ToolRegistry>, Arc<ToolExecutionPolicy>) {
    let mut reg = ToolRegistry::new();
    reg.register(Box::new(UpperTool));

    let policy = ToolExecutionPolicy {
        default_action: PolicyAction::PassThrough,
        rules: vec![PolicyRule {
            tool_name: "upper".to_string(),
            action: PolicyAction::Allow,
            timeout: None,
            max_concurrency: None,
        }],
    };

    (Arc::new(reg), Arc::new(policy))
}

#[test]
fn partition_allows_registered_tools() {
    let (reg, policy) = setup();
    let calls = vec![
        ToolCall {
            id: "1".to_string(),
            name: "upper".to_string(),
            input: serde_json::json!({"text": "hi"}),
        },
        ToolCall {
            id: "2".to_string(),
            name: "unknown".to_string(),
            input: serde_json::json!({}),
        },
    ];
    let (auto, pass) = partition_tool_calls(&calls, &reg, &policy);
    assert_eq!(auto.len(), 1);
    assert_eq!(auto[0].name, "upper");
    assert_eq!(pass.len(), 1);
    assert_eq!(pass[0].name, "unknown");
}

#[tokio::test]
async fn execute_registered_tool() {
    let (reg, policy) = setup();
    let config = LoopConfig::default();
    let calls = vec![ToolCall {
        id: "tc_1".to_string(),
        name: "upper".to_string(),
        input: serde_json::json!({"text": "hello world"}),
    }];

    let results = execute_tool_calls(&calls, &reg, &policy, &config).await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].tool_use_id, "tc_1");
    match &results[0].outcome {
        ToolOutcome::Success(v) => assert_eq!(v["result"], "HELLO WORLD"),
        other => panic!("expected Success, got {:?}", other),
    }
}

#[test]
fn tool_results_convert_to_user_message() {
    let results = vec![anyllm_proxy::tools::ToolResult {
        tool_use_id: "tc_1".to_string(),
        tool_name: "upper".to_string(),
        outcome: ToolOutcome::Success(serde_json::json!({"result": "HI"})),
    }];

    let msg = tool_results_to_user_message(&results);
    assert_eq!(msg.role, anyllm_translate::anthropic::Role::User);
    match &msg.content {
        anyllm_translate::anthropic::Content::Blocks(blocks) => {
            assert_eq!(blocks.len(), 1);
            match &blocks[0] {
                anyllm_translate::anthropic::ContentBlock::ToolResult {
                    tool_use_id,
                    is_error,
                    ..
                } => {
                    assert_eq!(tool_use_id, "tc_1");
                    assert_eq!(*is_error, Some(false));
                }
                other => panic!("expected ToolResult, got {:?}", other),
            }
        }
        other => panic!("expected Blocks, got {:?}", other),
    }
}

#[test]
fn duplicate_detection_works() {
    let a = vec![ToolCall {
        id: "1".to_string(),
        name: "upper".to_string(),
        input: serde_json::json!({"text": "same"}),
    }];
    let b = vec![ToolCall {
        id: "2".to_string(),
        name: "upper".to_string(),
        input: serde_json::json!({"text": "same"}),
    }];
    assert!(is_duplicate(&a, &b));

    let c = vec![ToolCall {
        id: "3".to_string(),
        name: "upper".to_string(),
        input: serde_json::json!({"text": "different"}),
    }];
    assert!(!is_duplicate(&a, &c));
}
```

- [ ] **Step 2: Run integration tests**

Run: `cargo test --test tool_execution -- -v`
Expected: All tests PASS

- [ ] **Step 3: Commit**

```bash
git add crates/proxy/tests/tool_execution.rs
git commit -m "test: add integration tests for tool execution engine"
```

---

### Task 14: Update CLAUDE.md and Final Verification

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Add tool execution to the "Working (verified)" section**

Add to CLAUDE.md under "Working (verified)":

```
- Tool execution engine: bounded loop with configurable max_iterations (default 1), per-tool policy (Allow/Deny/PassThrough), parallel execution via tokio::JoinSet, duplicate detection, timeout guards, and observability trace
- MCP server integration: SSE transport, tool discovery via tools/list, admin API (add/list/remove), config-file driven
- Builtin tools: execute_bash and read_file registered but PassThrough by default
```

- [ ] **Step 2: Add new config section documentation**

Add to the "Environment Variables" or "Configuration" section:

```
### Tool Execution Config (in PROXY_CONFIG simple format)

- `tool_execution.max_iterations`: Max LLM round-trips for tool execution loop (default: 1)
- `tool_execution.tool_timeout_secs`: Per-tool execution timeout (default: 30)
- `tool_execution.total_timeout_secs`: Wall-clock cap for entire loop (default: 300)
- `builtin_tools.<name>.enabled`: Enable/disable a builtin tool (default: true)
- `builtin_tools.<name>.policy`: allow | deny | pass_through (default: pass_through)
- `mcp_servers[].name`: MCP server identifier
- `mcp_servers[].url`: MCP server SSE endpoint URL
- `mcp_servers[].policy`: Default policy for all tools from this server
```

- [ ] **Step 3: Run full test suite**

Run: `cargo test && cargo clippy -- -D warnings`
Expected: All tests PASS, no clippy warnings

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: add tool execution engine and MCP integration to CLAUDE.md"
```
