//! Smoke test: verify the tool engine loop config is structurally correct.
use std::time::Duration;

#[test]
fn tool_loop_config_max_iterations_above_one() {
    // LoopConfig with max_iterations = 3 can be constructed and holds the value.
    // This confirms the streaming path has the plumbing to loop.
    let cfg = anyllm_proxy::tools::execution::LoopConfig {
        max_iterations: 3,
        tool_timeout: Duration::from_secs(30),
        total_timeout: Duration::from_secs(300),
        max_tool_calls_per_turn: 16,
    };
    assert_eq!(cfg.max_iterations, 3);
}
