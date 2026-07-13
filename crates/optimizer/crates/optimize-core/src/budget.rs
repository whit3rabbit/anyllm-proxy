//! Approximate target-LLM token counters. The heuristic (`bytes / 3.6`) needs no deps
//! and lives here so `optimize()` is usable standalone; the exact tiktoken counter for
//! OpenAI lives behind a feature in `anyllm_optimize_cli`. Report all counts as
//! estimates (ROADMAP risk 3: Anthropic's tokenizer is unpublished).

use crate::traits::BudgetCounter;

/// `bytes / divisor` heuristic. Default divisor 3.6 approximates English + code for both
/// OpenAI and Anthropic well enough for budget/cost math.
#[derive(Clone, Copy, Debug)]
pub struct HeuristicBudgetCounter {
    pub divisor: f64,
}

impl Default for HeuristicBudgetCounter {
    fn default() -> Self {
        Self { divisor: 3.6 }
    }
}

impl BudgetCounter for HeuristicBudgetCounter {
    fn count(&self, text: &str) -> u64 {
        (text.len() as f64 / self.divisor).ceil() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_with_length() {
        let c = HeuristicBudgetCounter::default();
        assert_eq!(c.count(""), 0);
        assert!(c.count("a".repeat(360).as_str()) == 100);
        assert!(c.count("hello world") > 0);
    }
}
