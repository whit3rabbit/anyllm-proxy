//! Orchestrator: the `optimize()` entry point. Fail-open is absolute — any `Err` or
//! panic in the pipeline forwards the original request untouched (invariant I1).

use std::hash::{Hash, Hasher};
use std::panic::AssertUnwindSafe;
use std::time::{Duration, Instant};

use crate::compress::compress_message;
use crate::cost::{net_cost_delta_usd, should_apply};
use crate::edit::EditScript;
use crate::error::OptimizeError;
use crate::frontier::frontier;
use crate::policy::{OptimizationPolicy, Policy};
use crate::render::{render, RenderedConversation};
use crate::report::{Mode, OptimizationReport};
use crate::traits::{BudgetCounter, CacheStrategy, TokenScorer};
use crate::types::{BufferId, ContentBlock, Conversation, Protection};
use crate::workspace::Workspace;

pub struct OptimizeOutcome {
    /// `None` ⇒ forward the original body (Off / Shadow / gate-skip / fail-open).
    pub rendered: Option<RenderedConversation>,
    pub report: OptimizationReport,
}

/// Wall-clock budget for the scorer across the WHOLE request (ALGO §6/§9). Checked
/// once per message, oldest-first, before that message is scored/compressed.
struct Deadline {
    start: Instant,
    budget: Duration,
}

impl Deadline {
    fn start(budget: Duration) -> Self {
        Self {
            start: Instant::now(),
            budget,
        }
    }

    fn expired(&self) -> bool {
        self.start.elapsed() >= self.budget
    }
}

/// Run FFEC over `conv`. See `ALGO.md §9`. Deterministic given the same inputs and
/// scorer artifact hash.
pub fn optimize(
    conv: &Conversation,
    policy: &Policy,
    strategy: &dyn CacheStrategy,
    scorer: &dyn TokenScorer,
    budget: &dyn BudgetCounter,
    ws: &mut Workspace,
) -> OptimizeOutcome {
    let version = policy.compression.version;
    let run = AssertUnwindSafe(|| run_inner(conv, policy, strategy, scorer, budget, ws));
    match std::panic::catch_unwind(run) {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(e)) => OptimizeOutcome {
            rendered: None,
            report: OptimizationReport::failed_open(policy.mode, version, e.to_string()),
        },
        Err(_) => OptimizeOutcome {
            rendered: None,
            report: OptimizationReport::failed_open(policy.mode, version, "panic in optimizer"),
        },
    }
}

/// Resolve `opt_policy` for `route` (per-route override precedence, see
/// `OptimizationPolicy::resolve`), then run [`optimize`]. This is the entry point the
/// proxy integration binds to so a route class can be turned `Off` independently of
/// others without callers threading the resolution logic themselves.
pub fn optimize_for_route(
    conv: &Conversation,
    opt_policy: &OptimizationPolicy,
    route: &str,
    strategy: &dyn CacheStrategy,
    scorer: &dyn TokenScorer,
    budget: &dyn BudgetCounter,
    ws: &mut Workspace,
) -> OptimizeOutcome {
    let policy = opt_policy.resolve(route);
    optimize(conv, &policy, strategy, scorer, budget, ws)
}

fn run_inner(
    conv: &Conversation,
    policy: &Policy,
    strategy: &dyn CacheStrategy,
    scorer: &dyn TokenScorer,
    budget: &dyn BudgetCounter,
    ws: &mut Workspace,
) -> Result<OptimizeOutcome, OptimizeError> {
    let n = conv.messages.len();
    let f = frontier(n, &policy.frontier);

    // 1. compress each eligible message (independent, pure), oldest-first (index 0 is
    // the oldest message, i.e. the highest-value/most-stable target — ALGO §6). A
    // scorer deadline bounds the WHOLE request: once it expires, remaining messages
    // get NO edits this turn (not scored at all) so they stay byte-verbatim and their
    // single verbatim→compressed transition is preserved for a later turn.
    let deadline = Deadline::start(policy.compression.deadline);
    let mut all_edits: Vec<(usize, BufferId, EditScript)> = Vec::new();
    let mut messages_compressed = 0u16;
    let mut messages_skipped_deadline = 0u16;
    for (i, msg) in conv.messages.iter().enumerate().take(f) {
        if msg.protection == Protection::Immutable || msg.client_cache_marker {
            continue;
        }
        if deadline.expired() {
            messages_skipped_deadline += 1;
            continue;
        }
        ws.clear();
        // M4.1 budget planner: if configured, plan this message's own ratio from its
        // own (role, absolute index, byte size) only — never from `n`, `f`, or any
        // other message — so a frozen message's planned ratio can't drift as later
        // turns get appended (I3). `compress_message` itself stays an unmodified pure
        // function of (msg, policy); we just hand it a per-message policy clone with
        // the ratio pre-planned, per ALGO's `compress_message(mi, P)` contract.
        let planned_policy;
        let compression_policy = match &policy.compression.planner {
            Some(planner) => {
                let byte_len: usize = msg.blocks.iter().map(block_byte_len).sum();
                let ratio = planner.plan_ratio(&policy.compression.ratios, msg.role, i, byte_len);
                let mut ratios = policy.compression.ratios.clone();
                ratios.set_text_ratio(msg.role, ratio);
                planned_policy = crate::policy::CompressionPolicy {
                    ratios,
                    ..policy.compression.clone()
                };
                &planned_policy
            }
            None => &policy.compression,
        };
        let scripts = compress_message(msg, compression_policy, scorer, ws)?;
        let mut touched = false;
        for (buf, script) in scripts {
            if let Some(src) = msg.buffer(buf) {
                if script.validate(src).is_ok() {
                    all_edits.push((i, buf, script));
                    touched = true;
                }
            }
        }
        if touched {
            messages_compressed += 1;
        }
    }

    // 2. cost estimation (ΔT, S) with the approximate budget counter.
    let s = frozen_zone_tokens(conv, f, budget);
    let mut dt = 0u64;
    let mut buf = String::new();
    for (mi, bid, script) in &all_edits {
        if let Some(src) = conv.messages[*mi].buffer(*bid) {
            let orig = budget.count(src);
            script.apply(src, &mut buf);
            dt += orig.saturating_sub(budget.count(&buf));
        }
    }

    // M4.3: a config-sourced `policy.pricing_override` wins over the strategy's own
    // (hardcoded-table) `pricing()`, so pricing can be versioned in config instead of
    // code (ROADMAP risk 5) without touching `CacheModel`/breakpoint placement, which
    // stay strategy-owned.
    let pricing = policy
        .pricing_override
        .unwrap_or_else(|| strategy.pricing());

    // 3. gate.
    let apply =
        !all_edits.is_empty() && should_apply(dt, s, policy.horizon, &pricing, &strategy.model());

    // 4. build report.
    let input_tokens_est = conversation_tokens(conv, budget);
    // Signed USD delta of applying vs skipping this transition (ALGO §9.1 / cost.rs).
    // No edits ⇒ nothing would change either way, so the honest delta is exactly 0.0
    // rather than the raw formula's (S·input·write_mult) rewrite-cost artifact at dt=0.
    let est_cost_delta_usd = if dt > 0 {
        net_cost_delta_usd(dt, s, policy.horizon, &pricing, &strategy.model())
    } else {
        0.0
    };
    let report = OptimizationReport {
        mode: policy.mode,
        applied: policy.mode == Mode::Live && apply,
        frontier: f,
        input_tokens_est,
        output_tokens_est: input_tokens_est.saturating_sub(if apply { dt } else { 0 }),
        removed_tokens_est: dt,
        rewrite_suffix_tokens: s,
        est_cost_delta_usd,
        scorer_ms: 0,
        messages_compressed,
        messages_skipped_deadline,
        decisions_hash: decisions_hash(&all_edits),
        policy_version: policy.compression.version,
        failure: None,
    };

    if policy.mode != Mode::Live || !apply {
        return Ok(OptimizeOutcome {
            rendered: None,
            report,
        });
    }
    let rendered = render(conv, &all_edits, strategy.breakpoint_at(f));
    Ok(OptimizeOutcome {
        rendered: Some(rendered),
        report,
    })
}

fn frozen_zone_tokens(conv: &Conversation, f: usize, budget: &dyn BudgetCounter) -> u64 {
    conv.messages
        .iter()
        .take(f)
        .flat_map(|m| m.blocks.iter())
        .map(|b| buffer_tokens(b, budget))
        .sum()
}

fn conversation_tokens(conv: &Conversation, budget: &dyn BudgetCounter) -> u64 {
    conv.messages
        .iter()
        .flat_map(|m| m.blocks.iter())
        .map(|b| buffer_tokens(b, budget))
        .sum()
}

fn buffer_tokens(b: &ContentBlock, budget: &dyn BudgetCounter) -> u64 {
    match b {
        ContentBlock::Text(s)
        | ContentBlock::ToolResult { raw: s }
        | ContentBlock::ToolUse { raw: s }
        | ContentBlock::Opaque { raw: s } => budget.count(s),
    }
}

/// Raw byte size of one block, for the budget planner's "size" dimension (M4.1). Uses
/// bytes rather than the target-LLM `BudgetCounter` so the planner never depends on
/// which provider/model is in play — only on the message's own bytes.
fn block_byte_len(b: &ContentBlock) -> usize {
    match b {
        ContentBlock::Text(s)
        | ContentBlock::ToolResult { raw: s }
        | ContentBlock::ToolUse { raw: s }
        | ContentBlock::Opaque { raw: s } => s.len(),
    }
}

/// Deterministic hash of all edit decisions (ranges + replacement text). Uses
/// `DefaultHasher` (fixed keys → stable across runs/threads/machines) for I2 auditing.
fn decisions_hash(edits: &[(usize, BufferId, EditScript)]) -> u64 {
    use crate::edit::Edit;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (mi, bid, script) in edits {
        mi.hash(&mut h);
        bid.0.hash(&mut h);
        for e in &script.edits {
            match e {
                Edit::Delete(r) => {
                    0u8.hash(&mut h);
                    r.start.hash(&mut h);
                    r.end.hash(&mut h);
                }
                Edit::Replace { range, text } => {
                    1u8.hash(&mut h);
                    range.start.hash(&mut h);
                    range.end.hash(&mut h);
                    text.hash(&mut h);
                }
            }
        }
    }
    h.finish()
}

#[cfg(test)]
#[path = "orchestrator_tests.rs"]
mod tests;
