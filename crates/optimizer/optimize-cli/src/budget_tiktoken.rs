//! M3.6: exact OpenAI token counting behind the `tiktoken` feature.
//!
//! `HeuristicBudgetCounter` (`bytes / 3.6`) is deliberately approximate everywhere else
//! (ROADMAP: no tokenizer perfectionism for budgets — Anthropic's tokenizer is
//! unpublished, so exactness there is impossible anyway). For OpenAI specifically the
//! real tokenizer IS available, so when this binary is built with `--features tiktoken`
//! the eval harness's own est_raw/est_comp columns can use it instead of the heuristic,
//! matching the wire counts an OpenAI-compatible endpoint reports. This mirrors
//! `anyllm_proxy`'s `count_tokens` endpoint, which already depends on `tiktoken-rs`
//! (o200k_base) for the same reason.

use anyllm_optimize_core::BudgetCounter;
use std::sync::LazyLock;
use tiktoken_rs::CoreBPE;

static TOKENIZER: LazyLock<CoreBPE> =
    LazyLock::new(|| tiktoken_rs::o200k_base().expect("failed to load o200k_base tokenizer"));

/// Exact `o200k_base` (GPT-4o family) token counter. Only meaningful for OpenAI-shaped
/// requests; Anthropic's tokenizer is unpublished so callers should keep using
/// `HeuristicBudgetCounter` there (ROADMAP risk 3).
#[derive(Clone, Copy, Debug, Default)]
pub struct TiktokenBudgetCounter;

impl BudgetCounter for TiktokenBudgetCounter {
    fn count(&self, text: &str) -> u64 {
        TOKENIZER.encode_ordinary(text).len() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_nonzero_for_nonempty_text() {
        let c = TiktokenBudgetCounter;
        assert_eq!(c.count(""), 0);
        assert!(c.count("hello world") > 0);
        // Longer text should never yield fewer tokens (BPE is monotone-ish for repeats).
        assert!(c.count(&"hello world ".repeat(20)) > c.count("hello world"));
    }
}
