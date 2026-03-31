use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::task::JoinSet;

use crate::tools::policy::{PolicyAction, ToolExecutionPolicy};
use crate::tools::registry::ToolRegistry;
use crate::tools::trace::{ToolOutcome};

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
    pub max_iterations: usize,
    pub tool_timeout: Duration,
    pub total_timeout: Duration,
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

/// Partition tool calls into those to auto-execute and those to pass through.
///
/// A tool is auto-executed only if it exists in the registry AND the policy says Allow.
/// Tools not in the registry always pass through regardless of policy.
/// Denied tools also pass through (caller is responsible for generating an error response).
pub fn partition_tool_calls<'a>(
    tool_calls: &'a [ToolCall],
    registry: &ToolRegistry,
    policy: &ToolExecutionPolicy,
) -> (Vec<&'a ToolCall>, Vec<&'a ToolCall>) {
    let mut auto_execute = Vec::new();
    let mut pass_through = Vec::new();

    for call in tool_calls {
        if registry.contains(&call.name) && policy.resolve(&call.name) == PolicyAction::Allow {
            auto_execute.push(call);
        } else {
            pass_through.push(call);
        }
    }

    (auto_execute, pass_through)
}

/// Execute tool calls in parallel, respecting per-tool timeouts.
///
/// Results are returned in the same order as `calls`.
pub async fn execute_tool_calls(
    calls: &[&ToolCall],
    registry: Arc<ToolRegistry>,
    policy: &ToolExecutionPolicy,
    config: &LoopConfig,
) -> Vec<ToolResult> {
    let capped = &calls[..calls.len().min(config.max_tool_calls_per_turn)];

    // Collect (original_index, ToolCall) to restore order after parallel execution.
    let indexed: Vec<(usize, &ToolCall)> = capped.iter().copied().enumerate().collect();

    let mut join_set: JoinSet<(usize, ToolResult)> = JoinSet::new();

    for (idx, call) in indexed {
        let timeout = policy
            .find_rule(&call.name)
            .and_then(|r| r.timeout)
            .unwrap_or(config.tool_timeout);

        let registry = Arc::clone(&registry);
        let id = call.id.clone();
        let name = call.name.clone();
        let input = call.input.clone();

        join_set.spawn(async move {
            let result = tokio::time::timeout(
                timeout,
                execute_single(&registry, &name, input),
            )
            .await;

            let outcome = match result {
                Ok(Ok(value)) => ToolOutcome::Success(value),
                Ok(Err(msg)) => ToolOutcome::Error {
                    message: msg,
                    retryable: false,
                },
                Err(_elapsed) => ToolOutcome::Timeout,
            };

            (
                idx,
                ToolResult {
                    tool_use_id: id,
                    tool_name: name,
                    outcome,
                },
            )
        });
    }

    let mut collected: Vec<(usize, ToolResult)> = Vec::with_capacity(capped.len());
    while let Some(res) = join_set.join_next().await {
        match res {
            Ok(pair) => collected.push(pair),
            Err(e) => {
                // JoinError means the task panicked; treat as an error outcome.
                // We don't have the index here, so we skip (shouldn't happen in practice).
                tracing::error!("tool execution task panicked: {e}");
            }
        }
    }

    collected.sort_by_key(|(idx, _)| *idx);
    collected.into_iter().map(|(_, r)| r).collect()
}

/// Check whether two slices of ToolCall represent the same logical calls.
///
/// Same length, same multiset of (name, input) pairs. IDs are ignored.
pub fn is_duplicate(a: &[ToolCall], b: &[ToolCall]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut a_pairs: Vec<(&str, &Value)> = a.iter().map(|c| (c.name.as_str(), &c.input)).collect();
    let mut b_pairs: Vec<(&str, &Value)> = b.iter().map(|c| (c.name.as_str(), &c.input)).collect();

    // Sort by name so comparison is order-independent.
    a_pairs.sort_by_key(|(name, _)| *name);
    b_pairs.sort_by_key(|(name, _)| *name);

    a_pairs == b_pairs
}

/// Execute a single tool by name, looking it up in the registry.
async fn execute_single(
    registry: &ToolRegistry,
    tool_name: &str,
    input: Value,
) -> Result<Value, String> {
    match registry.get(tool_name) {
        Some(tool) => tool.execute(input).await,
        None => Err(format!("tool '{}' not found in registry", tool_name)),
    }
}

// ---------------------------------------------------------------------------
// Timing wrapper used in the execution loop (available to callers)
// ---------------------------------------------------------------------------

/// Run `execute_tool_calls` and record wall-clock duration per call.
/// Returns (results, elapsed_per_call). Exposed for loop-level tracing.
pub async fn execute_tool_calls_timed(
    calls: &[&ToolCall],
    registry: Arc<ToolRegistry>,
    policy: &ToolExecutionPolicy,
    config: &LoopConfig,
) -> (Vec<ToolResult>, Duration) {
    let start = Instant::now();
    let results = execute_tool_calls(calls, registry, policy, config).await;
    (results, start.elapsed())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::policy::{PolicyAction, PolicyRule, ToolExecutionPolicy};
    use crate::tools::registry::ToolRegistry;
    use serde_json::json;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;

    // --- Test tool implementations ---

    struct EchoTool;

    impl crate::tools::registry::Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "Echoes input text in uppercase."
        }
        fn input_schema(&self) -> Value {
            json!({"type": "object", "properties": {"text": {"type": "string"}}})
        }
        fn execute<'a>(
            &'a self,
            input: Value,
        ) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send + 'a>> {
            Box::pin(async move {
                let text = input
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_uppercase();
                Ok(json!({"result": text}))
            })
        }
    }

    struct FailTool;

    impl crate::tools::registry::Tool for FailTool {
        fn name(&self) -> &str {
            "fail"
        }
        fn description(&self) -> &str {
            "Always returns an error."
        }
        fn input_schema(&self) -> Value {
            json!({"type": "object"})
        }
        fn execute<'a>(
            &'a self,
            _input: Value,
        ) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send + 'a>> {
            Box::pin(async move { Err("always fails".to_string()) })
        }
    }

    fn allow_policy(tool_name: &str) -> ToolExecutionPolicy {
        ToolExecutionPolicy {
            default_action: PolicyAction::PassThrough,
            rules: vec![PolicyRule {
                tool_name: tool_name.to_string(),
                action: PolicyAction::Allow,
                timeout: None,
                max_concurrency: None,
            }],
        }
    }

    fn passthrough_policy() -> ToolExecutionPolicy {
        ToolExecutionPolicy::default()
    }

    fn make_call(id: &str, name: &str, input: Value) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            input,
        }
    }

    // 1. passthrough policy -> all tools pass through
    #[test]
    fn partition_no_auto_execute() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool));
        let policy = passthrough_policy();

        let calls = vec![make_call("id1", "echo", json!({"text": "hi"}))];
        let (auto, pass) = partition_tool_calls(&calls, &registry, &policy);

        assert!(auto.is_empty());
        assert_eq!(pass.len(), 1);
    }

    // 2. allow policy + registered tool -> auto-execute; unregistered -> pass through
    #[test]
    fn partition_with_allow_policy() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool));
        let policy = allow_policy("echo");

        let calls = vec![
            make_call("id1", "echo", json!({"text": "hi"})),
            make_call("id2", "unknown_tool", json!({})),
        ];
        let (auto, pass) = partition_tool_calls(&calls, &registry, &policy);

        assert_eq!(auto.len(), 1);
        assert_eq!(auto[0].name, "echo");
        assert_eq!(pass.len(), 1);
        assert_eq!(pass[0].name, "unknown_tool");
    }

    // 3. EchoTool executes successfully
    #[tokio::test]
    async fn execute_tools_parallel_success() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool));
        let registry = Arc::new(registry);
        let policy = allow_policy("echo");
        let config = LoopConfig::default();

        let call = make_call("id1", "echo", json!({"text": "hello"}));
        let refs: Vec<&ToolCall> = vec![&call];

        let results = execute_tool_calls(&refs, registry, &policy, &config).await;

        assert_eq!(results.len(), 1);
        match &results[0].outcome {
            ToolOutcome::Success(v) => assert_eq!(v["result"], "HELLO"),
            other => panic!("expected Success, got {:?}", other),
        }
    }

    // 4. FailTool -> Error with "always fails"
    #[tokio::test]
    async fn execute_tools_parallel_failure() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(FailTool));
        let registry = Arc::new(registry);
        let policy = allow_policy("fail");
        let config = LoopConfig::default();

        let call = make_call("id2", "fail", json!({}));
        let refs: Vec<&ToolCall> = vec![&call];

        let results = execute_tool_calls(&refs, registry, &policy, &config).await;

        assert_eq!(results.len(), 1);
        match &results[0].outcome {
            ToolOutcome::Error { message, .. } => assert_eq!(message, "always fails"),
            other => panic!("expected Error, got {:?}", other),
        }
    }

    // 5. Same name+input, different IDs -> duplicate
    #[test]
    fn duplicate_detection_identifies_same_calls() {
        let a = vec![make_call("id1", "echo", json!({"text": "hi"}))];
        let b = vec![make_call("id2", "echo", json!({"text": "hi"}))];
        assert!(is_duplicate(&a, &b));
    }

    // 6. Same name, different input -> not duplicate
    #[test]
    fn duplicate_detection_different_args() {
        let a = vec![make_call("id1", "echo", json!({"text": "hello"}))];
        let b = vec![make_call("id2", "echo", json!({"text": "world"}))];
        assert!(!is_duplicate(&a, &b));
    }
}
