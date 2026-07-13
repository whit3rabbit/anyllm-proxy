//! Tool guardrail logic: partition tool calls into auto-execute / pass-through /
//! denied buckets, build denial + nudge results, and the combined
//! `partition_and_nudge` shared by both tool loops.

use std::collections::HashSet;

use super::{ToolCall, ToolResult};
use crate::tools::policy::{PolicyAction, ToolExecutionPolicy};
use crate::tools::registry::ToolRegistry;
use crate::tools::trace::ToolOutcome;
use crate::tools::{evaluate_tool_guardrails, ToolGuardrailNudge, ToolGuardrailRequestState};

/// Partition tool calls into three categories.
///
/// A tool call is only eligible for server-side policy evaluation when the
/// proxy advertised that exact tool name for this request. This prevents
/// client-supplied tool schemas from reusing privileged server tool names.
///
/// - `auto_execute`: proxy-advertised AND in registry AND policy says Allow
/// - `pass_through`: not proxy-advertised, not in registry, OR policy says PassThrough
/// - `denied`: proxy-advertised AND policy says Deny
pub fn partition_tool_calls<'a>(
    tool_calls: &'a [ToolCall],
    registry: &ToolRegistry,
    policy: &ToolExecutionPolicy,
    server_advertised_tool_names: &HashSet<String>,
) -> (Vec<&'a ToolCall>, Vec<&'a ToolCall>, Vec<&'a ToolCall>) {
    let mut auto_execute = Vec::new();
    let mut pass_through = Vec::new();
    let mut denied = Vec::new();

    for call in tool_calls {
        if !server_advertised_tool_names.contains(&call.name) {
            pass_through.push(call);
            continue;
        }

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

/// Build retryable ToolResults for guardrail nudges.
///
/// Only the tool calls a nudge targets get a result; every other call in the
/// batch is left for normal execution/pass-through. `calls` supplies the tool
/// name for each nudged `call_id`.
pub fn guardrail_nudge_results(
    calls: &[ToolCall],
    nudges: &[ToolGuardrailNudge],
) -> Vec<ToolResult> {
    nudges
        .iter()
        .filter_map(|nudge| {
            let call = calls.iter().find(|c| c.id == nudge.call_id)?;
            Some(ToolResult {
                tool_use_id: call.id.clone(),
                tool_name: call.name.clone(),
                outcome: ToolOutcome::Error {
                    message: format!("[ToolCallPolicyNudge] {}", nudge.content),
                    retryable: true,
                },
            })
        })
        .collect()
}

/// Partition `tool_calls`, then evaluate guardrail nudges only against the
/// calls that would otherwise be auto-executed by this proxy.
///
/// A nudge target must be something the proxy actually owns: guardrails are
/// an advisory retry mechanism that answers the model on the proxy's behalf
/// (a synthetic tool_result + a follow-up backend call), so applying it to a
/// pass-through call (not in the registry / not proxy-advertised, meaning the
/// *caller* owns and expects to execute it -- e.g. a Claude-Code-style client's
/// own Bash/Grep/Edit/Write tool) would silently swallow that call's real
/// tool_use turn instead of returning it to the caller. Restricting
/// `evaluate_tool_guardrails` to the post-partition `auto_exec` set keeps
/// nudges impossible to apply to anything the proxy doesn't own, and
/// `denied` never needs nudge-filtering since nudges only ever originate
/// from `auto_exec` now (the two sets are disjoint by construction).
///
/// Shared by the streaming and non-streaming tool loops so a fix here can't
/// drift between the two copies.
pub fn partition_and_nudge<'a>(
    tool_calls: &'a [ToolCall],
    tool_specs: &[anyllm_translate::anthropic::Tool],
    registry: &ToolRegistry,
    policy: &ToolExecutionPolicy,
    server_advertised_tool_names: &HashSet<String>,
    guardrails: &crate::tools::ToolGuardrailConfig,
    guardrail_state: &mut ToolGuardrailRequestState,
) -> (Vec<&'a ToolCall>, Vec<ToolResult>, Vec<ToolResult>) {
    let (auto_exec, _pass_through, denied) =
        partition_tool_calls(tool_calls, registry, policy, server_advertised_tool_names);

    let auto_exec_owned: Vec<ToolCall> = auto_exec.iter().map(|c| (*c).clone()).collect();
    let nudges =
        evaluate_tool_guardrails(&auto_exec_owned, tool_specs, guardrails, guardrail_state);
    let nudged_ids: HashSet<&str> = nudges.iter().map(|n| n.call_id.as_str()).collect();
    let nudge_results = guardrail_nudge_results(&auto_exec_owned, &nudges);

    let auto_exec: Vec<&ToolCall> = auto_exec
        .into_iter()
        .filter(|c| !nudged_ids.contains(c.id.as_str()))
        .collect();
    let denied_results = denied_tool_results(&denied);

    (auto_exec, nudge_results, denied_results)
}
