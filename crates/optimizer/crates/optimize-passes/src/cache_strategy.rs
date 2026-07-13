//! Per-provider `CacheStrategy` impls. Anthropic owns explicit breakpoints (place the
//! deepest at the frontier); OpenAI/Gemini are implicit-prefix (no breakpoint). Disabled
//! never applies (zero pricing → the cost gate returns false).

use anyllm_optimize_core::{CacheModel, CacheStrategy, Pricing};

use crate::cost_gate::{anthropic_pricing, gemini_pricing, openai_pricing};

/// Anthropic: optimizer-managed explicit breakpoints. Frozen region == cached region;
/// frontier advances never invalidate.
pub struct AnthropicStrategy {
    pub pricing: Pricing,
}

impl Default for AnthropicStrategy {
    fn default() -> Self {
        Self {
            pricing: anthropic_pricing(),
        }
    }
}

impl CacheStrategy for AnthropicStrategy {
    fn pricing(&self) -> Pricing {
        self.pricing
    }
    fn model(&self) -> CacheModel {
        CacheModel::ExplicitBreakpoints
    }
    fn breakpoint_at(&self, frontier: usize) -> Option<usize> {
        Some(frontier)
    }
}

/// OpenAI: implicit prefix cache. Frontier advance rewrites the suffix once; the cost
/// gate (with horizon) decides if it pays.
pub struct OpenAiStrategy {
    pub pricing: Pricing,
}

impl Default for OpenAiStrategy {
    fn default() -> Self {
        Self {
            pricing: openai_pricing(),
        }
    }
}

impl CacheStrategy for OpenAiStrategy {
    fn pricing(&self) -> Pricing {
        self.pricing
    }
    fn model(&self) -> CacheModel {
        CacheModel::ImplicitPrefix
    }
    fn breakpoint_at(&self, _frontier: usize) -> Option<usize> {
        None
    }
}

/// Gemini: implicit prefix cache (same regime as OpenAI here).
pub struct GeminiStrategy {
    pub pricing: Pricing,
}

impl Default for GeminiStrategy {
    fn default() -> Self {
        Self {
            pricing: gemini_pricing(),
        }
    }
}

impl CacheStrategy for GeminiStrategy {
    fn pricing(&self) -> Pricing {
        self.pricing
    }
    fn model(&self) -> CacheModel {
        CacheModel::ImplicitPrefix
    }
    fn breakpoint_at(&self, _frontier: usize) -> Option<usize> {
        None
    }
}

/// No caching assumptions; zero pricing so the cost gate always skips. Use when a
/// provider's cache behavior is unknown and you want compression disabled by cost math.
#[derive(Default)]
pub struct DisabledStrategy;

impl CacheStrategy for DisabledStrategy {
    fn pricing(&self) -> Pricing {
        Pricing {
            input: 0.0,
            cached_read: 0.0,
            cache_write_mult: 1.0,
        }
    }
    fn model(&self) -> CacheModel {
        CacheModel::ImplicitPrefix
    }
    fn breakpoint_at(&self, _frontier: usize) -> Option<usize> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_places_breakpoint_at_frontier() {
        assert_eq!(AnthropicStrategy::default().breakpoint_at(7), Some(7));
        assert_eq!(OpenAiStrategy::default().breakpoint_at(7), None);
    }
}
