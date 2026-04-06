use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

/// Result of executing a single tool call.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolOutcome {
    /// Tool completed and returned a JSON value.
    Success(Value),
    /// Tool returned an error. `retryable` signals whether the LLM should retry.
    Error { message: String, retryable: bool },
    /// Tool exceeded its per-call timeout.
    Timeout,
}

/// Why the tool execution loop terminated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminationReason {
    /// LLM returned a response with no tool calls (normal completion).
    NoToolCalls,
    /// The loop hit `max_iterations` before the LLM stopped calling tools.
    MaxIterations,
    /// Total wall-clock time exceeded `total_timeout_secs`.
    Timeout,
    /// A duplicate tool call was detected (same name + args seen in a previous iteration).
    DuplicateDetected,
    /// Every tool call in the last iteration failed or timed out.
    AllToolsFailed,
}

/// Observability record for a single tool call within one loop iteration.
#[derive(Debug, Clone, Serialize)]
pub struct ToolCallTrace {
    pub tool_name: String,
    pub duration: Duration,
    pub outcome: ToolOutcome,
}

/// Observability record for one LLM round-trip + tool execution batch.
#[derive(Debug, Clone, Serialize)]
pub struct IterationTrace {
    pub tool_calls: Vec<ToolCallTrace>,
    /// Time spent waiting for the LLM response (not including tool execution).
    pub llm_latency: Duration,
}

/// Full observability trace for an entire tool execution loop.
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_trace_reports_no_tool_calls() {
        let trace = LoopTrace {
            iterations: vec![],
            total_duration: Duration::from_millis(0),
            termination_reason: TerminationReason::NoToolCalls,
        };
        assert_eq!(trace.total_tool_calls(), 0);
    }

    #[test]
    fn trace_counts_tool_calls_across_iterations() {
        let make_call = |name: &str| ToolCallTrace {
            tool_name: name.to_string(),
            duration: Duration::from_millis(10),
            outcome: ToolOutcome::Success(json!({})),
        };

        let trace = LoopTrace {
            iterations: vec![
                IterationTrace {
                    tool_calls: vec![make_call("tool_a")],
                    llm_latency: Duration::from_millis(100),
                },
                IterationTrace {
                    tool_calls: vec![make_call("tool_b")],
                    llm_latency: Duration::from_millis(100),
                },
            ],
            total_duration: Duration::from_millis(300),
            termination_reason: TerminationReason::MaxIterations,
        };
        assert_eq!(trace.total_tool_calls(), 2);
    }

    #[test]
    fn tool_outcome_serializes_to_json() {
        // Success
        let success = ToolOutcome::Success(json!({"key": "val"}));
        let v = serde_json::to_value(&success).unwrap();
        assert_eq!(v["type"], "success");

        // Error
        let error = ToolOutcome::Error {
            message: "something broke".to_string(),
            retryable: true,
        };
        let v = serde_json::to_value(&error).unwrap();
        assert_eq!(v["type"], "error");
        assert_eq!(v["retryable"], true);
        assert_eq!(v["message"], "something broke");

        // Timeout
        let timeout = ToolOutcome::Timeout;
        let v = serde_json::to_value(&timeout).unwrap();
        assert_eq!(v["type"], "timeout");
    }
}
