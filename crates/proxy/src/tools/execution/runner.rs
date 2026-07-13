//! Core async tool-execution runners and the centralized bounded tool loop
//! for non-streaming requests.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::task::JoinSet;

use super::{
    extract_tool_calls, partition_and_nudge, response_to_assistant_message,
    tool_results_to_user_message, LoopConfig, ToolCall, ToolEngineState, ToolResult,
};
use crate::tools::policy::ToolExecutionPolicy;
use crate::tools::registry::ToolRegistry;
use crate::tools::trace::{
    IterationTrace, LoopTrace, TerminationReason, ToolCallTrace, ToolOutcome,
};
use crate::tools::ToolGuardrailRequestState;

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
            let result =
                tokio::time::timeout(timeout, execute_single(&registry, &name, input)).await;

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
///
/// `guardrails` is the effective guardrail config for this request (the
/// runtime-tunable override applied on top of `engine.guardrails`, the
/// static per-process preset -- see `crate::tools::resolve_runtime_guardrails`
/// and `AppState::effective_tool_guardrails`). Callers that don't need the
/// runtime override can pass `&engine.guardrails` directly.
pub async fn maybe_execute_tools<F, Fut>(
    engine: &ToolEngineState,
    original_req: &anyllm_translate::anthropic::MessageCreateRequest,
    server_advertised_tool_names: &HashSet<String>,
    initial_response: anyllm_translate::anthropic::MessageResponse,
    guardrails: &crate::tools::ToolGuardrailConfig,
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
    let mut guardrail_state = ToolGuardrailRequestState::new();

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

        // Advisory guardrails: each nudge targets one offending call and comes
        // back as a retryable error result. Nudged calls are skipped this turn;
        // every other call is partitioned and executed as usual. Each distinct
        // decision nudges only once per request (across iterations), so a
        // model that ignores a nudge and repeats the call lets it proceed
        // instead of spinning the loop to max_iterations. Nudges only ever
        // apply to calls this proxy would actually execute -- see
        // `partition_and_nudge`'s doc comment.
        let (auto_exec, nudge_results, denied_results) = partition_and_nudge(
            &tool_calls,
            original_req.tools.as_deref().unwrap_or(&[]),
            &engine.registry,
            &engine.policy,
            server_advertised_tool_names,
            guardrails,
            &mut guardrail_state,
        );

        if auto_exec.is_empty() && denied_results.is_empty() && nudge_results.is_empty() {
            return (
                current_response,
                LoopTrace {
                    iterations,
                    total_duration: loop_start.elapsed(),
                    termination_reason: TerminationReason::NoToolCalls,
                },
            );
        }

        // If there is nothing to execute (only nudges and/or denials), send the
        // advisory/error results back to the LLM immediately without running a tool.
        if auto_exec.is_empty() {
            let mut results = nudge_results;
            results.extend(denied_results);
            current_messages.push(response_to_assistant_message(&current_response));
            current_messages.push(tool_results_to_user_message(&results));
            let mut follow_up_req = original_req.clone();
            follow_up_req.messages = current_messages.clone();
            let llm_start = Instant::now();
            let traces: Vec<ToolCallTrace> = results
                .iter()
                .map(|r| ToolCallTrace {
                    tool_name: r.tool_name.clone(),
                    duration: Duration::ZERO,
                    outcome: r.outcome.clone(),
                })
                .collect();
            iterations.push(IterationTrace {
                tool_calls: traces,
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
                    tracing::warn!(error = %e, "follow-up backend call failed after nudge/deny");
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

        // Append nudge + denied-tool error results so the LLM sees all outcomes.
        results.extend(nudge_results);
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
