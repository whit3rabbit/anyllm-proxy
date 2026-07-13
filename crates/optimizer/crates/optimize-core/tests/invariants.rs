//! Property tests for the FFEC invariants (ALGO §10). Written against `UniformScorer`
//! (deterministic, no ML) so they run in milestone 1.
//!
//! Covered here: I2 determinism, I3 frozen-stability, I4 monotone-frontier,
//! I7 extractive-subsequence, I8 ratio-honesty. (I1 fail-open and I5/I6 protected/validity
//! are covered by unit tests in the crate and the passes pipeline test.)

use anyllm_optimize_core::{
    compress_message, frontier, optimize, BudgetPlanner, CacheModel, CacheStrategy,
    CompressionPolicy, ContentBlock, Conversation, FrontierPolicy, HeuristicBudgetCounter, Message,
    Mode, Policy, Pricing, Protection, Role, UniformScorer, Workspace,
};
use proptest::prelude::*;

struct ExplicitStrategy;
impl CacheStrategy for ExplicitStrategy {
    fn pricing(&self) -> Pricing {
        Pricing {
            input: 3.0,
            cached_read: 0.3,
            cache_write_mult: 1.25,
        }
    }
    fn model(&self) -> CacheModel {
        CacheModel::ExplicitBreakpoints
    }
    fn breakpoint_at(&self, f: usize) -> Option<usize> {
        Some(f)
    }
}

fn word() -> impl Strategy<Value = String> {
    prop::sample::select(
        &[
            "the", "quick", "brown", "fox", "jumps", "over", "lazy", "dog", "river", "green",
            "field", "mountain", "toward", "distant", "blue", "again", "across", "wide", "trees",
            "beyond",
        ][..],
    )
    .prop_map(|s| s.to_string())
}

/// A code-like token, deliberately containing punctuation but never a backtick or tilde
/// (so it can never accidentally form its own fence delimiter).
fn code_token() -> impl Strategy<Value = String> {
    prop::sample::select(
        &[
            "let", "x", "=", "42;", "fn", "foo()", "{", "}", "return", "y", "if", "else", "while",
            "i++;", "print(x)", "self.val",
        ][..],
    )
    .prop_map(|s| s.to_string())
}

fn code_line() -> impl Strategy<Value = String> {
    prop::collection::vec(code_token(), 1..6).prop_map(|v| v.join(" "))
}

/// A prose message long enough to exceed `min_len` so compression actually engages.
fn message() -> impl Strategy<Value = Message> {
    (
        prop::collection::vec(word(), 60..90),
        prop_oneof![Just(Role::User), Just(Role::Assistant)],
    )
        .prop_map(|(words, role)| Message {
            role,
            blocks: vec![ContentBlock::Text(words.join(" "))],
            protection: Protection::Mutable,
            client_cache_marker: false,
        })
}

fn conversation() -> impl Strategy<Value = Conversation> {
    prop::collection::vec(message(), 1..30).prop_map(Conversation::new)
}

fn live_policy() -> Policy {
    Policy {
        mode: Mode::Live,
        ..Default::default()
    }
}

/// Same as `live_policy` but with the M4.1 budget planner enabled (nonzero age/size
/// steps), so the proptests below exercise the planner's ratio path, not just the
/// default no-op.
fn live_policy_with_planner() -> Policy {
    let mut policy = live_policy();
    policy.compression.planner = Some(BudgetPlanner {
        age_step: 0.01,
        size_step: 0.001,
        size_unit: 50,
        min_ratio: 0.1,
    });
    policy
}

fn text_of(m: &anyllm_optimize_core::RenderedMessage) -> String {
    m.blocks
        .iter()
        .map(|b| match b {
            ContentBlock::Text(s) | ContentBlock::ToolResult { raw: s } => s.as_str(),
            _ => "",
        })
        .collect::<Vec<_>>()
        .join("\u{0}")
}

fn words_vec(s: &str) -> Vec<&str> {
    s.split_whitespace().collect()
}

/// True iff `sub` is a subsequence of `sup` (word level).
fn is_subsequence(sub: &[&str], sup: &[&str]) -> bool {
    let mut it = sup.iter();
    sub.iter().all(|w| it.any(|x| x == w))
}

proptest! {
    // I2: determinism — same input, same decisions across runs.
    #[test]
    fn i2_determinism(conv in conversation()) {
        let mut ws = Workspace::new();
        let budget = HeuristicBudgetCounter::default();
        let a = optimize(&conv, &live_policy(), &ExplicitStrategy, &UniformScorer, &budget, &mut ws);
        let b = optimize(&conv, &live_policy(), &ExplicitStrategy, &UniformScorer, &budget, &mut ws);
        prop_assert_eq!(a.report.decisions_hash, b.report.decisions_hash);
    }

    // I4: monotone frontier.
    #[test]
    fn i4_monotone_frontier(n in 0usize..300, keep in 0usize..8, k in 1usize..8) {
        let p = FrontierPolicy { keep_recent: keep, batch_k: k };
        prop_assert!(frontier(n + 1, &p) >= frontier(n, &p));
    }

    // I7: extractive — every rendered word occurs in the input in order (per message).
    #[test]
    fn i7_extractive_subsequence(conv in conversation()) {
        let mut ws = Workspace::new();
        let budget = HeuristicBudgetCounter::default();
        let out = optimize(&conv, &live_policy(), &ExplicitStrategy, &UniformScorer, &budget, &mut ws);
        if let Some(rendered) = out.rendered {
            for (i, rmsg) in rendered.messages.iter().enumerate() {
                let orig = conv.messages[i]
                    .blocks
                    .iter()
                    .map(|b| match b { ContentBlock::Text(s) => s.as_str(), _ => "" })
                    .collect::<Vec<_>>()
                    .join(" ");
                let out_text = text_of(rmsg);
                let ow = words_vec(&orig);
                let nw = words_vec(&out_text);
                prop_assert!(is_subsequence(&nw, &ow),
                    "message {} output not a subsequence of input", i);
            }
        }
    }

    // I3: frozen stability — appending a turn does not change bytes of messages that were
    // already behind the frontier.
    #[test]
    fn i3_frozen_stability(conv in conversation(), extra in message()) {
        let mut ws = Workspace::new();
        let budget = HeuristicBudgetCounter::default();
        let f_old = frontier(conv.messages.len(), &Policy::default().frontier);

        let before = optimize(&conv, &live_policy(), &ExplicitStrategy, &UniformScorer, &budget, &mut ws);

        let mut conv2 = conv.clone();
        conv2.messages.push(extra);
        let after = optimize(&conv2, &live_policy(), &ExplicitStrategy, &UniformScorer, &budget, &mut ws);

        if let (Some(a), Some(b)) = (before.rendered, after.rendered) {
            for i in 0..f_old {
                prop_assert_eq!(text_of(&a.messages[i]), text_of(&b.messages[i]),
                    "frozen message {} changed after append", i);
            }
        }
    }

    // I8: ratio honesty — kept words are within [forced, ceil(ratio*n)] bounds, so output
    // is never longer than input (word count).
    #[test]
    fn i8_ratio_honesty(conv in conversation()) {
        let mut ws = Workspace::new();
        let budget = HeuristicBudgetCounter::default();
        let out = optimize(&conv, &live_policy(), &ExplicitStrategy, &UniformScorer, &budget, &mut ws);
        if let Some(rendered) = out.rendered {
            for (i, rmsg) in rendered.messages.iter().enumerate() {
                let orig = match &conv.messages[i].blocks[0] {
                    ContentBlock::Text(s) => s.clone(),
                    _ => String::new(),
                };
                let out_text = text_of(rmsg);
                prop_assert!(words_vec(&out_text).len() <= words_vec(&orig).len());
            }
        }
    }

    // I6: fence pairing — a fenced code block's bytes (both delimiter lines and the
    // content between them) survive `compress_message` byte-for-byte, and the fence
    // marker count in the output always matches the input, regardless of how much
    // surrounding prose gets compressed away.
    #[test]
    fn i6_fence_pairing_preserved(
        prefix in prop::collection::vec(word(), 40..60),
        code_lines in prop::collection::vec(code_line(), 3..10),
        suffix in prop::collection::vec(word(), 40..60),
    ) {
        let code = code_lines.join("\n");
        let text = format!("{}\n```\n{}\n```\n{}", prefix.join(" "), code, suffix.join(" "));

        let msg = Message {
            role: Role::User,
            blocks: vec![ContentBlock::Text(text.clone())],
            protection: Protection::Mutable,
            client_cache_marker: false,
        };
        let mut ws = Workspace::new();
        let edits = compress_message(&msg, &CompressionPolicy::default(), &UniformScorer, &mut ws)
            .unwrap();

        let fence_start = text.find("```").unwrap();
        let fence_end = text.rfind("```").unwrap() + 3;
        let fenced_src = &text[fence_start..fence_end];

        let out_text = if let Some((_, script)) = edits.first() {
            let mut out = String::new();
            script.apply(&text, &mut out);
            out
        } else {
            text.clone()
        };

        prop_assert_eq!(
            out_text.matches("```").count(),
            text.matches("```").count(),
            "fence marker count must be preserved"
        );
        prop_assert!(
            out_text.contains(fenced_src),
            "fenced block bytes must survive compression unchanged"
        );
    }

    // M4.1 budget planner, I3: frozen stability holds even with age/size-planned ratios
    // enabled. A message's `index` (its input to `plan_ratio`) never changes when later
    // turns are appended, so a message already behind the OLD frontier must render
    // byte-identical bytes before and after the append — exactly like `i3_frozen_stability`,
    // but exercising the planner's ratio path instead of the flat default.
    #[test]
    fn i3_budget_planner_frozen_stability(conv in conversation(), extra in message()) {
        let mut ws = Workspace::new();
        let budget = HeuristicBudgetCounter::default();
        let policy = live_policy_with_planner();
        let f_old = frontier(conv.messages.len(), &policy.frontier);

        let before = optimize(&conv, &policy, &ExplicitStrategy, &UniformScorer, &budget, &mut ws);

        let mut conv2 = conv.clone();
        conv2.messages.push(extra);
        let after = optimize(&conv2, &policy, &ExplicitStrategy, &UniformScorer, &budget, &mut ws);

        if let (Some(a), Some(b)) = (before.rendered, after.rendered) {
            for i in 0..f_old {
                prop_assert_eq!(text_of(&a.messages[i]), text_of(&b.messages[i]),
                    "frozen message {} changed after append with budget planner enabled", i);
            }
        }
    }

    // M4.1 budget planner, I5/I7: planner-adjusted ratios never turn compression into
    // anything but extractive deletion — output stays a word-level subsequence of input,
    // same as the flat-ratio path (`i7_extractive_subsequence`).
    #[test]
    fn i5_budget_planner_extractive_subsequence(conv in conversation()) {
        let mut ws = Workspace::new();
        let budget = HeuristicBudgetCounter::default();
        let out = optimize(&conv, &live_policy_with_planner(), &ExplicitStrategy, &UniformScorer, &budget, &mut ws);
        if let Some(rendered) = out.rendered {
            for (i, rmsg) in rendered.messages.iter().enumerate() {
                let orig = conv.messages[i]
                    .blocks
                    .iter()
                    .map(|b| match b { ContentBlock::Text(s) => s.as_str(), _ => "" })
                    .collect::<Vec<_>>()
                    .join(" ");
                let out_text = text_of(rmsg);
                let ow = words_vec(&orig);
                let nw = words_vec(&out_text);
                prop_assert!(is_subsequence(&nw, &ow),
                    "message {} output not a subsequence of input with budget planner enabled", i);
            }
        }
    }
}
