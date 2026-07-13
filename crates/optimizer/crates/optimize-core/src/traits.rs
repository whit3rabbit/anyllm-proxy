//! Core traits and the cost-model value types they reference.
//!
//! `TokenScorer` scores; the planner (in passes) decides; `CacheStrategy` (per provider)
//! constrains and supplies pricing. `BudgetCounter` approximates target-LLM token counts
//! for budget/cost math (never conflated with the scorer's exact tokenizer).

use crate::edit::EditScript;
use crate::error::ScoreError;
use crate::types::{BufferId, Conversation};
use crate::workspace::Workspace;

/// Importance scorer: one `p_preserve` per input word. MUST be deterministic for
/// identical input on the same `artifact_hash`.
pub trait TokenScorer: Send + Sync {
    /// `words.len() == result.len()`.
    fn score_words(&self, words: &[&str], ws: &mut Workspace) -> Result<Vec<f32>, ScoreError>;
    /// Folded into `PolicyVersion` so a model swap forces a deliberate cache re-write.
    fn artifact_hash(&self) -> u64;
}

/// Fallback scorer: everything mid-importance. Selection degenerates to forced-keeps +
/// first-k. Used for fail-open and the no-ML skeleton — never silently for "better"
/// results.
#[derive(Clone, Copy, Debug, Default)]
pub struct UniformScorer;

impl TokenScorer for UniformScorer {
    fn score_words(&self, words: &[&str], _ws: &mut Workspace) -> Result<Vec<f32>, ScoreError> {
        Ok(vec![0.5; words.len()])
    }
    fn artifact_hash(&self) -> u64 {
        0
    }
}

/// A compression pass over a conversation, emitting per-buffer edit scripts. Passes are
/// pure and order-independent per message (see `ALGO.md §5`).
pub trait Pass {
    fn name(&self) -> &'static str;
    fn plan(
        &self,
        conv: &Conversation,
        scorer: &dyn TokenScorer,
        ws: &mut Workspace,
    ) -> Result<Vec<(usize, BufferId, EditScript)>, ScoreError>;
}

/// Provider pricing (per Mtok). `cache_write_mult` is Anthropic ~1.25 (5-min TTL);
/// OpenAI/Gemini implicit ≈ 1.0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pricing {
    pub input: f64,
    pub cached_read: f64,
    pub cache_write_mult: f64,
}

/// Error parsing a [`Pricing`] table out of a config string (`Pricing::from_config_str`).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PricingConfigError {
    #[error("missing required pricing key `{0}`")]
    MissingKey(&'static str),
    #[error("invalid value for pricing key `{key}`: {value}")]
    InvalidValue { key: String, value: String },
}

impl Pricing {
    /// Parse a `Pricing` table from a minimal `key=value` config format: one assignment
    /// per line, blank lines and `#`-prefixed comments ignored. All three keys
    /// (`input`, `cached_read`, `cache_write_mult`) are required — this is deliberately
    /// dependency-free (no serde; `optimize-core` has none) so pricing can be sourced
    /// from config instead of the hardcoded per-provider tables in
    /// `anyllm_optimize_passes::cost_gate` (ROADMAP risk 5: "the cost gate depends on
    /// pricing tables that change; version them in config, not code"). Richer formats
    /// (TOML/JSON/YAML) parse into this same shape one layer up, in `optimize-passes` or
    /// the proxy, where serde is already a dependency.
    pub fn from_config_str(s: &str) -> Result<Self, PricingConfigError> {
        let mut input = None;
        let mut cached_read = None;
        let mut cache_write_mult = None;
        for line in s.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            let parsed: f64 = value
                .parse()
                .map_err(|_| PricingConfigError::InvalidValue {
                    key: key.to_string(),
                    value: value.to_string(),
                })?;
            match key {
                "input" => input = Some(parsed),
                "cached_read" => cached_read = Some(parsed),
                "cache_write_mult" => cache_write_mult = Some(parsed),
                _ => {}
            }
        }
        Ok(Self {
            input: input.ok_or(PricingConfigError::MissingKey("input"))?,
            cached_read: cached_read.ok_or(PricingConfigError::MissingKey("cached_read"))?,
            cache_write_mult: cache_write_mult
                .ok_or(PricingConfigError::MissingKey("cache_write_mult"))?,
        })
    }
}

#[cfg(test)]
mod pricing_config_tests {
    use super::*;

    #[test]
    fn parses_well_formed_table() {
        let p = Pricing::from_config_str(
            "# anthropic\ninput = 3.00\ncached_read=0.30\ncache_write_mult=1.25\n",
        )
        .unwrap();
        assert_eq!(
            p,
            Pricing {
                input: 3.00,
                cached_read: 0.30,
                cache_write_mult: 1.25,
            }
        );
    }

    #[test]
    fn missing_key_errors() {
        let err = Pricing::from_config_str("input=1.0\ncached_read=0.5\n").unwrap_err();
        assert_eq!(err, PricingConfigError::MissingKey("cache_write_mult"));
    }

    #[test]
    fn invalid_value_errors() {
        let err = Pricing::from_config_str("input=nope\ncached_read=0.5\ncache_write_mult=1.0\n")
            .unwrap_err();
        assert_eq!(
            err,
            PricingConfigError::InvalidValue {
                key: "input".to_string(),
                value: "nope".to_string(),
            }
        );
    }
}

/// How the provider caches, which decides whether a frontier advance ever invalidates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheModel {
    /// Optimizer owns breakpoints; the recent zone is never cached, so transitions never
    /// invalidate. Compression is ≈ always cost-positive (gate only checks ΔT > 0).
    ExplicitBreakpoints,
    /// Provider auto-caches everything, so a frontier advance rewrites the suffix once.
    ImplicitPrefix,
}

/// Per-provider strategy: frontier/breakpoint rules, cache model, and pricing for the
/// cost gate. Impls live in `anyllm_optimize_passes`.
pub trait CacheStrategy {
    fn pricing(&self) -> Pricing;
    fn model(&self) -> CacheModel;
    /// Byte offset (in the rendered conversation) at which to place the deepest cache
    /// breakpoint, if the provider supports explicit breakpoints; else `None`.
    fn breakpoint_at(&self, frontier: usize) -> Option<usize>;
}

/// Approximate target-LLM token counter for budget/cost math. Exactness unnecessary.
pub trait BudgetCounter: Send + Sync {
    fn count(&self, text: &str) -> u64;
}
