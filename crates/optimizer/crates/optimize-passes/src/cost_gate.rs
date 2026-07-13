//! Pricing tables + re-export of the pure cost gate. Prices are placeholders; version
//! them in config, not code (ROADMAP risk 5). Units: USD per Mtok.

use anyllm_optimize_core::Pricing;

pub use anyllm_optimize_core::{net_cost_delta_usd, should_apply};

/// OpenAI implicit-prefix pricing (cached read ~50% off, no write premium).
pub fn openai_pricing() -> Pricing {
    Pricing {
        input: 2.50,
        cached_read: 1.25,
        cache_write_mult: 1.0,
    }
}

/// Anthropic explicit-breakpoint pricing (cached read ~0.1x, write ~1.25x, 5-min TTL).
pub fn anthropic_pricing() -> Pricing {
    Pricing {
        input: 3.00,
        cached_read: 0.30,
        cache_write_mult: 1.25,
    }
}

/// Gemini implicit-prefix pricing (placeholder).
pub fn gemini_pricing() -> Pricing {
    Pricing {
        input: 1.25,
        cached_read: 0.31,
        cache_write_mult: 1.0,
    }
}
