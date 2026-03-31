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

/// Partition tool calls into three categories.
///
/// - `auto_execute`: in registry AND policy says Allow
/// - `pass_through`: not in registry, OR policy says PassThrough
/// - `denied`: policy says Deny (caller must surface error ToolResults to LLM)
pub fn partition_tool_calls<'a>(
    tool_calls: &'a [ToolCall],
    registry: &ToolRegistry,
    policy: &ToolExecutionPolicy,
) -> (Vec<&'a ToolCall>, Vec<&'a ToolCall>, Vec<&'a ToolCall>) {
    let mut auto_execute = Vec::new();
    let mut pass_through = Vec::new();
    let mut denied = Vec::new();

    for call in tool_calls {
        match policy.resolve(&call.name) {
            PolicyAction::Deny => denied.push(call),
            PolicyAction::Allow if registry.contains(&call.name) => auto_execute.push(call),
            // Allow but not in registry, or PassThrough
            _ => pass_through.push(call),
        }
    }

    (auto_execute, pass_through, denied)
}

/// Build error ToolResults for denied tool calls.
pub fn denied_tool_results(denied: &[&ToolCall]) -> Vec<ToolResult> {
    denied
        .iter()
        .map(|call| ToolResult {
            tool_use_id: call.id.clone(),
            tool_name: call.name.clone(),
            outcome: ToolOutcome::Error {
                message: format!("Tool '{}' is denied by policy", call.name),
                retryable: false,
            },
        })
        .collect()
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
// Helper functions for extracting tool calls and building follow-up messages
// ---------------------------------------------------------------------------

/// Extract ToolCall structs from an Anthropic MessageResponse.
pub fn extract_tool_calls(
    response: &anyllm_translate::anthropic::MessageResponse,
) -> Vec<ToolCall> {
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

/// Convert tool execution results to an Anthropic user message with ToolResult blocks.
pub fn tool_results_to_user_message(
    results: &[ToolResult],
) -> anyllm_translate::anthropic::InputMessage {
    let blocks: Vec<anyllm_translate::anthropic::ContentBlock> = results
        .iter()
        .map(|r| {
            let (content_text, is_error) = match &r.outcome {
                ToolOutcome::Success(v) => (serde_json::to_string(v).unwrap_or_default(), false),
                ToolOutcome::Error { message, .. } => (message.clone(), true),
                ToolOutcome::Timeout => ("Tool execution timed out".to_string(), true),
            };
            anyllm_translate::anthropic::ContentBlock::ToolResult {
                tool_use_id: r.tool_use_id.clone(),
                content: Some(anyllm_translate::anthropic::ToolResultContent::Text(
                    content_text,
                )),
                is_error: Some(is_error),
            }
        })
        .collect();

    anyllm_translate::anthropic::InputMessage {
        role: anyllm_translate::anthropic::Role::User,
        content: anyllm_translate::anthropic::Content::Blocks(blocks),
    }
}

/// Convert a MessageResponse's content into an assistant InputMessage for conversation history.
pub fn response_to_assistant_message(
    response: &anyllm_translate::anthropic::MessageResponse,
) -> anyllm_translate::anthropic::InputMessage {
    anyllm_translate::anthropic::InputMessage {
        role: anyllm_translate::anthropic::Role::Assistant,
        content: anyllm_translate::anthropic::Content::Blocks(response.content.clone()),
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

// ---------------------------------------------------------------------------
// Centralized bounded tool-execution loop for non-streaming requests
// ---------------------------------------------------------------------------

use crate::tools::trace::{IterationTrace, LoopTrace, TerminationReason, ToolCallTrace};

/// Engine state needed by `maybe_execute_tools`. Re-exported alias so callers
/// do not need to reach into `server::routes`.
pub use crate::server::routes::ToolEngineState;

/// Process an LLM response for tool execution. If auto-executable tool calls
/// are found, executes them and makes follow-up backend calls in a bounded loop.
///
/// Returns the final response (original if no tools were auto-executed) and a
/// `LoopTrace` recording what happened.
///
/// `backend_call` is a closure the caller provides. It takes a
/// `MessageCreateRequest` and returns the translated `MessageResponse`.
/// This keeps the loop backend-agnostic: the handler knows how to translate
/// and call its specific backend; this function only knows about Anthropic types.
pub async fn maybe_execute_tools<F, Fut>(
    engine: &ToolEngineState,
    original_req: &anyllm_translate::anthropic::MessageCreateRequest,
    initial_response: anyllm_translate::anthropic::MessageResponse,
    backend_call: F,
) -> (anyllm_translate::anthropic::MessageResponse, LoopTrace)
where
    F: Fn(anyllm_translate::anthropic::MessageCreateRequest) -> Fut,
    Fut: std::future::Future<Output = Result<anyllm_translate::anthropic::MessageResponse, String>>,
{
    let loop_start = Instant::now();
    let mut iterations: Vec<IterationTrace> = Vec::new();
    let mut current_response = initial_response;
    let mut current_messages = original_req.messages.clone();
    let mut prev_tool_calls: Option<Vec<ToolCall>> = None;

    for _iteration in 0..engine.loop_config.max_iterations {
        // Guard: total timeout
        if loop_start.elapsed() > engine.loop_config.total_timeout {
            return (
                current_response,
                LoopTrace {
                    iterations,
                    total_duration: loop_start.elapsed(),
                    termination_reason: TerminationReason::Timeout,
                },
            );
        }

        let tool_calls = extract_tool_calls(&current_response);
        let (auto_exec, _pass_through, denied) =
            partition_tool_calls(&tool_calls, &engine.registry, &engine.policy);

        // Generate error results for denied tools so the LLM sees them.
        let denied_results = denied_tool_results(&denied);

        if auto_exec.is_empty() && denied_results.is_empty() {
            return (
                current_response,
                LoopTrace {
                    iterations,
                    total_duration: loop_start.elapsed(),
                    termination_reason: TerminationReason::NoToolCalls,
                },
            );
        }

        // If only denials (no auto-execute), send error results back to LLM immediately.
        if auto_exec.is_empty() {
            current_messages.push(response_to_assistant_message(&current_response));
            current_messages.push(tool_results_to_user_message(&denied_results));
            let mut follow_up_req = original_req.clone();
            follow_up_req.messages = current_messages.clone();
            let llm_start = Instant::now();
            let deny_traces: Vec<ToolCallTrace> = denied_results
                .iter()
                .map(|r| ToolCallTrace {
                    tool_name: r.tool_name.clone(),
                    duration: Duration::ZERO,
                    outcome: r.outcome.clone(),
                })
                .collect();
            iterations.push(IterationTrace {
                tool_calls: deny_traces,
                llm_latency: Duration::ZERO,
            });
            match backend_call(follow_up_req).await {
                Ok(resp) => {
                    if let Some(last) = iterations.last_mut() {
                        last.llm_latency = llm_start.elapsed();
                    }
                    prev_tool_calls = None;
                    current_response = resp;
                    continue;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "follow-up backend call failed after deny");
                    if let Some(last) = iterations.last_mut() {
                        last.llm_latency = llm_start.elapsed();
                    }
                    return (
                        current_response,
                        LoopTrace {
                            iterations,
                            total_duration: loop_start.elapsed(),
                            termination_reason: TerminationReason::NoToolCalls,
                        },
                    );
                }
            }
        }

        // Guard: duplicate detection (same tool calls as previous iteration)
        let auto_calls: Vec<ToolCall> = auto_exec.iter().map(|c| (*c).clone()).collect();
        if let Some(ref prev) = prev_tool_calls {
            if is_duplicate(prev, &auto_calls) {
                return (
                    current_response,
                    LoopTrace {
                        iterations,
                        total_duration: loop_start.elapsed(),
                        termination_reason: TerminationReason::DuplicateDetected,
                    },
                );
            }
        }

        // Execute auto-allowed tools in parallel
        let exec_start = Instant::now();
        let mut results = execute_tool_calls(
            &auto_exec,
            engine.registry.clone(),
            &engine.policy,
            &engine.loop_config,
        )
        .await;
        let exec_duration = exec_start.elapsed();

        // Append denied-tool error results so the LLM sees all outcomes.
        results.extend(denied_results);

        // Build per-tool traces
        let tool_traces: Vec<ToolCallTrace> = results
            .iter()
            .map(|r| ToolCallTrace {
                tool_name: r.tool_name.clone(),
                duration: exec_duration,
                outcome: r.outcome.clone(),
            })
            .collect();

        // Guard: all tools failed (includes deny errors, which are non-retryable)
        let all_failed = results
            .iter()
            .all(|r| !matches!(r.outcome, ToolOutcome::Success(_)));

        iterations.push(IterationTrace {
            tool_calls: tool_traces,
            llm_latency: Duration::ZERO, // filled after backend call below
        });

        if all_failed {
            return (
                current_response,
                LoopTrace {
                    iterations,
                    total_duration: loop_start.elapsed(),
                    termination_reason: TerminationReason::AllToolsFailed,
                },
            );
        }

        // Build follow-up: append assistant response + tool results to conversation
        current_messages.push(response_to_assistant_message(&current_response));
        current_messages.push(tool_results_to_user_message(&results));

        let mut follow_up_req = original_req.clone();
        follow_up_req.messages = current_messages.clone();

        // Call backend via caller-provided closure
        let llm_start = Instant::now();
        match backend_call(follow_up_req).await {
            Ok(resp) => {
                if let Some(last) = iterations.last_mut() {
                    last.llm_latency = llm_start.elapsed();
                }
                tracing::info!(
                    tools_executed = results.len(),
                    iteration = _iteration + 1,
                    "tool execution loop: iteration complete"
                );
                prev_tool_calls = Some(auto_calls);
                current_response = resp;
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "follow-up backend call failed, returning last good response"
                );
                if let Some(last) = iterations.last_mut() {
                    last.llm_latency = llm_start.elapsed();
                }
                return (
                    current_response,
                    LoopTrace {
                        iterations,
                        total_duration: loop_start.elapsed(),
                        // Backend error is not a clean termination; closest match is NoToolCalls
                        // since we are stopping the loop and returning what we have.
                        termination_reason: TerminationReason::NoToolCalls,
                    },
                );
            }
        }
    }

    // Exhausted max_iterations
    (
        current_response,
        LoopTrace {
            iterations,
            total_duration: loop_start.elapsed(),
            termination_reason: TerminationReason::MaxIterations,
        },
    )
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
        let (auto, pass, denied) = partition_tool_calls(&calls, &registry, &policy);

        assert!(auto.is_empty());
        assert_eq!(pass.len(), 1);
        assert!(denied.is_empty());
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
        let (auto, pass, denied) = partition_tool_calls(&calls, &registry, &policy);

        assert_eq!(auto.len(), 1);
        assert_eq!(auto[0].name, "echo");
        assert_eq!(pass.len(), 1);
        assert_eq!(pass[0].name, "unknown_tool");
        assert!(denied.is_empty());
    }

    // 2b. deny policy -> tool goes to denied bucket, not pass_through
    #[test]
    fn partition_deny_policy() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool));
        let policy = ToolExecutionPolicy {
            default_action: PolicyAction::PassThrough,
            rules: vec![PolicyRule {
                tool_name: "echo".to_string(),
                action: PolicyAction::Deny,
                timeout: None,
                max_concurrency: None,
            }],
        };

        let calls = vec![make_call("id1", "echo", json!({"text": "hi"}))];
        let (auto, pass, denied) = partition_tool_calls(&calls, &registry, &policy);

        assert!(auto.is_empty());
        assert!(pass.is_empty());
        assert_eq!(denied.len(), 1);
        assert_eq!(denied[0].name, "echo");
    }

    // 2c. denied_tool_results generates error ToolResult with correct message
    #[test]
    fn denied_tool_results_generates_error_results() {
        let calls = vec![make_call("id1", "rm_rf", json!({}))];
        let refs: Vec<&ToolCall> = calls.iter().collect();
        let results = denied_tool_results(&refs);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].tool_use_id, "id1");
        assert_eq!(results[0].tool_name, "rm_rf");
        match &results[0].outcome {
            ToolOutcome::Error { message, retryable } => {
                assert!(message.contains("rm_rf"));
                assert!(message.contains("denied"));
                assert!(!retryable);
            }
            other => panic!("expected Error outcome, got {:?}", other),
        }
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

    // 7. extract_tool_calls picks up ToolUse blocks
    #[test]
    fn extract_tool_calls_finds_tool_use_blocks() {
        use anyllm_translate::anthropic::{ContentBlock, MessageResponse, Role, StopReason, Usage};

        let resp = MessageResponse {
            id: "msg_1".into(),
            response_type: "message".into(),
            role: Role::Assistant,
            content: vec![
                ContentBlock::Text {
                    text: "Let me call a tool.".into(),
                },
                ContentBlock::ToolUse {
                    id: "tu_1".into(),
                    name: "echo".into(),
                    input: json!({"text": "hello"}),
                },
                ContentBlock::ToolUse {
                    id: "tu_2".into(),
                    name: "search".into(),
                    input: json!({"query": "rust"}),
                },
            ],
            model: "test".into(),
            stop_reason: Some(StopReason::ToolUse),
            stop_sequence: None,
            usage: Usage {
                input_tokens: 10,
                output_tokens: 20,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
            created: None,
        };

        let calls = extract_tool_calls(&resp);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "echo");
        assert_eq!(calls[0].id, "tu_1");
        assert_eq!(calls[1].name, "search");
        assert_eq!(calls[1].id, "tu_2");
    }

    // 8. extract_tool_calls returns empty vec when no ToolUse blocks
    #[test]
    fn extract_tool_calls_empty_when_no_tool_use() {
        use anyllm_translate::anthropic::{ContentBlock, MessageResponse, Role, StopReason, Usage};

        let resp = MessageResponse {
            id: "msg_2".into(),
            response_type: "message".into(),
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "Just text, no tools.".into(),
            }],
            model: "test".into(),
            stop_reason: Some(StopReason::EndTurn),
            stop_sequence: None,
            usage: Usage {
                input_tokens: 5,
                output_tokens: 10,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
            created: None,
        };

        let calls = extract_tool_calls(&resp);
        assert!(calls.is_empty());
    }
}
