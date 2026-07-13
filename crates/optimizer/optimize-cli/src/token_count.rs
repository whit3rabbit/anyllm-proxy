use super::{Api, Args};
use anyllm_optimize_core::{BudgetCounter, HeuristicBudgetCounter};
use serde_json::Value;

/// M3.6: selects the counter `count_local` uses to report est_raw/est_comp. Defaults to
/// `HeuristicBudgetCounter` (bytes/3.6, provider-agnostic). When this binary is built with
/// `--features tiktoken` AND the target is OpenAI-shaped, uses the exact `o200k_base`
/// tokenizer instead — Anthropic's tokenizer is unpublished, so the heuristic remains the
/// only option there (ROADMAP risk 3). This only changes what the harness *reports*; the
/// planning counter `compress()` passes into `optimize()` is unchanged.
#[cfg_attr(not(feature = "tiktoken"), allow(unused_variables))]
pub fn build_counter(args: &Args) -> Box<dyn BudgetCounter> {
    #[cfg(feature = "tiktoken")]
    if args.api == Api::Openai {
        return Box::new(super::budget_tiktoken::TiktokenBudgetCounter);
    }
    Box::new(HeuristicBudgetCounter::default())
}

/// Local token estimate: sum the budget counter over all message text content.
pub fn count_local(body: &Value, api: Api, counter: &dyn BudgetCounter) -> u64 {
    let mut total = 0u64;
    if api == Api::Anthropic {
        if let Some(sys) = body.get("system") {
            total += count_content(sys, counter);
        }
    }
    if let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) {
        for m in msgs {
            if let Some(c) = m.get("content") {
                total += count_content(c, counter);
            }
        }
    }
    total
}

pub fn count_content(c: &Value, counter: &dyn BudgetCounter) -> u64 {
    match c {
        Value::String(s) => counter.count(s),
        Value::Array(parts) => parts
            .iter()
            .map(|p| match p.get("text").and_then(|t| t.as_str()) {
                Some(t) => counter.count(t),
                None => counter.count(&p.to_string()),
            })
            .sum(),
        _ => 0,
    }
}
