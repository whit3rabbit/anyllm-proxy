use super::*;
use crate::budget::HeuristicBudgetCounter;
use crate::traits::{CacheModel, Pricing, UniformScorer};
use crate::types::{Message, Role};

struct TestStrategy;
impl CacheStrategy for TestStrategy {
    fn pricing(&self) -> Pricing {
        Pricing {
            input: 1.0,
            cached_read: 0.5,
            cache_write_mult: 1.0,
        }
    }
    fn model(&self) -> CacheModel {
        CacheModel::ExplicitBreakpoints
    }
    fn breakpoint_at(&self, frontier: usize) -> Option<usize> {
        Some(frontier)
    }
}

fn long_user(text: &str) -> Message {
    Message {
        role: Role::User,
        blocks: vec![ContentBlock::Text(text.into())],
        protection: Protection::Mutable,
        client_cache_marker: false,
    }
}

fn convo(n: usize) -> Conversation {
    let long = "The quick brown fox jumps over the lazy dog again and again across \
                the wide green field toward the distant blue mountains beyond the river \
                and the tall dark trees under a bright and cloudless summer sky at noon."
        .to_string();
    Conversation::new((0..n).map(|_| long_user(&long)).collect())
}

#[test]
fn shadow_never_renders_but_reports() {
    let conv = convo(20);
    let policy = Policy {
        mode: Mode::Shadow,
        ..Default::default()
    };
    let mut ws = Workspace::new();
    let out = optimize(
        &conv,
        &policy,
        &TestStrategy,
        &UniformScorer,
        &HeuristicBudgetCounter::default(),
        &mut ws,
    );
    assert!(out.rendered.is_none());
    assert_eq!(out.report.mode, Mode::Shadow);
    assert!(out.report.frontier > 0);
    assert!(out.report.removed_tokens_est > 0);
}

#[test]
fn live_renders_and_is_deterministic() {
    let conv = convo(20);
    let policy = Policy {
        mode: Mode::Live,
        ..Default::default()
    };
    let mut ws = Workspace::new();
    let a = optimize(
        &conv,
        &policy,
        &TestStrategy,
        &UniformScorer,
        &HeuristicBudgetCounter::default(),
        &mut ws,
    );
    let b = optimize(
        &conv,
        &policy,
        &TestStrategy,
        &UniformScorer,
        &HeuristicBudgetCounter::default(),
        &mut ws,
    );
    assert!(a.rendered.is_some());
    assert_eq!(a.report.decisions_hash, b.report.decisions_hash);
    assert!(a.report.applied);
}

#[test]
fn cost_delta_is_signed_and_nonzero_for_compression() {
    let conv = convo(20);
    let policy = Policy {
        mode: Mode::Live,
        ..Default::default()
    };
    let mut ws = Workspace::new();
    let out = optimize(
        &conv,
        &policy,
        &TestStrategy,
        &UniformScorer,
        &HeuristicBudgetCounter::default(),
        &mut ws,
    );
    assert!(out.report.applied);
    assert!(out.report.removed_tokens_est > 0);
    let expected = net_cost_delta_usd(
        out.report.removed_tokens_est,
        out.report.rewrite_suffix_tokens,
        policy.horizon,
        &TestStrategy.pricing(),
        &TestStrategy.model(),
    );
    assert!(expected > 0.0, "fixture should have a positive delta");
    assert_eq!(out.report.est_cost_delta_usd, expected);
}

#[test]
fn cost_delta_is_zero_for_noop() {
    // No message reaches `min_len`, so no edits are produced and dt stays 0 — the
    // report's delta must be exactly 0.0, not the raw formula's rewrite-cost artifact.
    let conv = Conversation::new(vec![long_user("too short to compress")]);
    let policy = Policy {
        mode: Mode::Live,
        ..Default::default()
    };
    let mut ws = Workspace::new();
    let out = optimize(
        &conv,
        &policy,
        &TestStrategy,
        &UniformScorer,
        &HeuristicBudgetCounter::default(),
        &mut ws,
    );
    assert!(!out.report.applied);
    assert_eq!(out.report.removed_tokens_est, 0);
    assert_eq!(out.report.est_cost_delta_usd, 0.0);
}

#[test]
fn route_override_turns_off_one_route_while_another_still_compresses() {
    use crate::policy::{OptimizationPolicy, RouteOverride};
    use std::collections::HashMap;

    let conv = convo(20);
    let mut routes = HashMap::new();
    routes.insert(
        "batch".to_string(),
        RouteOverride {
            mode: Some(Mode::Off),
            ratios: None,
            pricing: None,
        },
    );
    let opt_policy = OptimizationPolicy {
        mode: Mode::Live,
        routes,
        ..OptimizationPolicy::default()
    };

    // Overridden route: rendered stays None, i.e. output ≡ input for that route.
    let mut ws = Workspace::new();
    let off = optimize_for_route(
        &conv,
        &opt_policy,
        "batch",
        &TestStrategy,
        &UniformScorer,
        &HeuristicBudgetCounter::default(),
        &mut ws,
    );
    assert!(off.rendered.is_none());
    assert!(!off.report.applied);

    // Unlisted route: falls back to the top-level Live default and still compresses.
    let mut ws2 = Workspace::new();
    let on = optimize_for_route(
        &conv,
        &opt_policy,
        "interactive",
        &TestStrategy,
        &UniformScorer,
        &HeuristicBudgetCounter::default(),
        &mut ws2,
    );
    assert!(on.rendered.is_some());
    assert!(on.report.applied);
}

/// Scores like `UniformScorer` but sleeps on its very first call, so the deadline
/// check ahead of every later message is guaranteed to observe it expired
/// regardless of scheduler jitter (sleep duration >> deadline budget below).
struct SlowFirstCallScorer {
    calls: std::sync::atomic::AtomicUsize,
}
impl TokenScorer for SlowFirstCallScorer {
    fn score_words(
        &self,
        words: &[&str],
        _ws: &mut Workspace,
    ) -> Result<Vec<f32>, crate::error::ScoreError> {
        if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(vec![0.5; words.len()])
    }
    fn artifact_hash(&self) -> u64 {
        0
    }
}

#[test]
fn deadline_expiry_leaves_later_messages_byte_identical() {
    // 20 messages -> frontier eligible_end=16 (keep_recent=4), all long enough to
    // clear `min_len`. A 50ms sleep on the first scorer call vs. a 10ms deadline
    // guarantees message 0 is scored before the deadline and every later message
    // observes it already expired (elapsed only ever grows).
    let conv = convo(20);
    let mut policy = Policy {
        mode: Mode::Live,
        ..Default::default()
    };
    policy.compression.deadline = Duration::from_millis(10);

    let scorer = SlowFirstCallScorer {
        calls: std::sync::atomic::AtomicUsize::new(0),
    };
    let mut ws = Workspace::new();
    let out = optimize(
        &conv,
        &policy,
        &TestStrategy,
        &scorer,
        &HeuristicBudgetCounter::default(),
        &mut ws,
    );

    assert!(
        out.report.messages_compressed >= 1,
        "message scored before the deadline should be compressed"
    );
    assert!(
        out.report.messages_skipped_deadline >= 1,
        "messages after deadline expiry must be counted as skipped"
    );
    assert_eq!(
        out.report.messages_compressed as usize + out.report.messages_skipped_deadline as usize,
        out.report.frontier,
        "every eligible message is either compressed or explicitly deadline-skipped, never silently dropped"
    );

    let rendered = out
        .rendered
        .expect("gate should apply given a nonzero saving");
    let compressed = out.report.messages_compressed as usize;
    // Deadline-skipped messages must render byte-identical to the source.
    for i in compressed..out.report.frontier {
        assert_eq!(
            rendered.messages[i].blocks[0], conv.messages[i].blocks[0],
            "deadline-skipped message {i} must stay byte-identical"
        );
    }
    // The message scored before the deadline actually got edited.
    assert_ne!(
        rendered.messages[0].blocks[0], conv.messages[0].blocks[0],
        "the message scored before the deadline should have been edited"
    );
}

/// M4.3: `Policy::pricing_override`, loaded from a config string via
/// `Pricing::from_config_str` (no hardcoded-table constant), must be what the
/// orchestrator actually uses for the cost gate/report — not `TestStrategy`'s own
/// (hardcoded-in-code) `pricing()`. Proven by asserting the report's dollar delta
/// matches `net_cost_delta_usd` computed with the config pricing and differs from
/// the value that would result from `TestStrategy::pricing()` alone.
#[test]
fn pricing_comes_from_config() {
    use crate::traits::Pricing;

    let conv = convo(20);

    // Deliberately far from `TestStrategy::pricing()` (input:1.0, cached_read:0.5,
    // cache_write_mult:1.0) and from any of `anyllm_optimize_passes::cost_gate`'s
    // hardcoded tables — proves the number really came from this config string.
    let config_pricing =
        Pricing::from_config_str("input=9.0\ncached_read=0.05\ncache_write_mult=2.0\n")
            .expect("well-formed config parses");
    assert_ne!(config_pricing, TestStrategy.pricing());

    let policy = Policy {
        mode: Mode::Live,
        pricing_override: Some(config_pricing),
        ..Default::default()
    };
    let mut ws = Workspace::new();
    let out = optimize(
        &conv,
        &policy,
        &TestStrategy,
        &UniformScorer,
        &HeuristicBudgetCounter::default(),
        &mut ws,
    );
    assert!(out.report.applied);
    assert!(out.report.removed_tokens_est > 0);

    let expected_from_config = net_cost_delta_usd(
        out.report.removed_tokens_est,
        out.report.rewrite_suffix_tokens,
        policy.horizon,
        &config_pricing,
        &TestStrategy.model(),
    );
    let would_be_from_hardcoded_strategy = net_cost_delta_usd(
        out.report.removed_tokens_est,
        out.report.rewrite_suffix_tokens,
        policy.horizon,
        &TestStrategy.pricing(),
        &TestStrategy.model(),
    );

    assert_eq!(out.report.est_cost_delta_usd, expected_from_config);
    assert_ne!(
        out.report.est_cost_delta_usd, would_be_from_hardcoded_strategy,
        "orchestrator must use the config-loaded pricing, not the strategy's hardcoded table"
    );
}

#[test]
fn empty_conversation_is_noop() {
    let conv = Conversation::default();
    let out = optimize(
        &conv,
        &Policy::default(),
        &TestStrategy,
        &UniformScorer,
        &HeuristicBudgetCounter::default(),
        &mut Workspace::new(),
    );
    assert!(out.rendered.is_none());
    assert_eq!(out.report.frontier, 0);
}
