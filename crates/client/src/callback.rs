//! Application-side `success_callback` hook.
//!
//! One typed callback fired exactly once per successful
//! [`Client::messages`](crate::Client::messages) call, carrying the original
//! Anthropic request, the translated Anthropic response, the resolved backend
//! model, the wall-clock duration, and (when pricing is configured) the
//! computed cost in USD.
//!
//! Design rationale and tradeoffs are documented in
//! `docs/COMPARISON_LITELM.md` (Adopt / Adopt (deferred)) and the client
//! changelog. Streaming completion and failure hooks are explicitly out of
//! scope for v1; v2 if requested.

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use anyllm_translate::anthropic::{MessageCreateRequest, MessageResponse};

/// Payload delivered to the user-supplied [`SuccessCallback`] after a
/// successful non-streaming completion.
#[derive(Debug, Clone)]
pub struct CallbackInput {
    /// The Anthropic Messages API request the user supplied to `messages()`.
    pub request: MessageCreateRequest,
    /// The translated Anthropic Messages API response.
    pub response: MessageResponse,
    /// The model name from the *request* (e.g. `"claude-sonnet-4-6"`).
    pub request_model: String,
    /// The backend model name resolved by translation
    /// (e.g. `"gpt-4o"`, after `TranslationConfig::model_map`).
    pub backend_model: String,
    /// Wall-clock duration from `messages()` start to response receipt.
    pub duration: Duration,
    /// Estimated cost in USD, or `None` if either (a) no pricing was
    /// configured or (b) the backend model has no pricing entry.
    pub cost_usd: Option<f64>,
}

/// A user-supplied hook fired after each successful completion.
///
/// Implementations must be `Send + Sync` and `'static` because the underlying
/// `Client` is `Clone` and the callback may be invoked from any cloned
/// instance. Panics inside the callback are caught by the SDK and logged;
/// they never propagate to the caller of `messages()`.
pub type SuccessCallback = Arc<dyn Fn(CallbackInput) + Send + Sync + 'static>;

/// Per-million-token pricing in USD. `(input_per_million, output_per_million)`.
///
/// This is the same shape the proxy's `cost::ModelPricing` exposes, minus the
/// bundled JSON DB — the SDK does not pull in a pricing table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelPrice {
    /// USD per 1 million input tokens.
    pub input_per_million: f64,
    /// USD per 1 million output tokens.
    pub output_per_million: f64,
}

/// Optional pricing table used to populate [`CallbackInput::cost_usd`].
///
/// Lookups are exact-match. If the backend model is not present, `cost_usd`
/// is `None` and a single warning is logged per unknown model id (deduped
/// via [`CallbackContext`]) — same dedup pattern the proxy uses for billing
/// leaks.
#[derive(Default, Clone)]
pub struct PricingConfig {
    prices: HashMap<String, ModelPrice>,
}

impl PricingConfig {
    /// Create an empty pricing config (cost is always `None`).
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or overwrite a single entry. Returns `self` for chaining.
    pub fn with_price(
        mut self,
        model: impl Into<String>,
        input_per_million: f64,
        output_per_million: f64,
    ) -> Self {
        self.prices.insert(
            model.into(),
            ModelPrice {
                input_per_million,
                output_per_million,
            },
        );
        self
    }

    /// Replace the entire table. Useful for bulk-loaded pricing.
    pub fn with_prices(mut self, prices: HashMap<String, ModelPrice>) -> Self {
        self.prices = prices;
        self
    }

    /// Compute cost in USD for `model` + usage, or `None` if no entry.
    pub fn cost_for_usage(
        &self,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
    ) -> Option<f64> {
        let price = self.prices.get(model)?;
        let cost = (input_tokens as f64 / 1_000_000.0) * price.input_per_million
            + (output_tokens as f64 / 1_000_000.0) * price.output_per_million;
        Some(cost)
    }

    /// Number of priced models. Useful for tests/diagnostics.
    pub fn len(&self) -> usize {
        self.prices.len()
    }

    /// Whether the pricing table is empty.
    pub fn is_empty(&self) -> bool {
        self.prices.is_empty()
    }
}

impl fmt::Debug for PricingConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PricingConfig")
            .field("entries", &self.prices.len())
            .finish()
    }
}

/// Shared, deduplicated state across all clones of a `Client`.
///
/// Holds the user-supplied success callback (if any), the pricing table, and a
/// small set of model ids that have already been logged as having no pricing
/// entry (so a buggy pricing config doesn't spam logs once per request).
#[derive(Default)]
pub(crate) struct CallbackContext {
    pub callback: Option<SuccessCallback>,
    pub pricing: PricingConfig,
    pub warned_missing_pricing: std::sync::Mutex<HashSet<String>>,
}

impl CallbackContext {
    pub(crate) fn fire(&self, input: CallbackInput) {
        let Some(cb) = self.callback.as_ref() else {
            return;
        };
        // Catch panics so a buggy callback never breaks the SDK's success path.
        // AssertUnwindSafe is sound here: the closure only mutates its own state.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cb(input);
        }));
        if let Err(payload) = result {
            tracing::error!(
                panic = %payload_panic_message(&payload),
                "anyllm_client: success_callback panicked; SDK response already returned to caller"
            );
        }
    }

    pub(crate) fn cost_for(
        &self,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
    ) -> Option<f64> {
        if self.pricing.is_empty() {
            return None;
        }
        if let Some(cost) = self
            .pricing
            .cost_for_usage(model, input_tokens, output_tokens)
        {
            return Some(cost);
        }
        // Dedup: warn at most once per unknown model id.
        if let Ok(mut warned) = self.warned_missing_pricing.lock() {
            if warned.insert(model.to_string()) {
                tracing::warn!(
                    model = model,
                    "anyllm_client: no pricing entry for backend model; cost_usd will be None"
                );
            }
        }
        None
    }
}

impl fmt::Debug for CallbackContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CallbackContext")
            .field("has_callback", &self.callback.is_some())
            .field("pricing_entries", &self.pricing.len())
            .finish()
    }
}

/// Best-effort stringification of a panic payload. Works for the common
/// `&str` / `String` cases and falls back to the type name for exotic payloads.
fn payload_panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    "<non-string panic payload>".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pricing_config_computes_correctly() {
        let pricing = PricingConfig::new()
            .with_price("gpt-4o", 2.50, 10.00)
            .with_price("gpt-4o-mini", 0.15, 0.60);

        // 1M input tokens @ $2.50/M = $2.50; 500K output @ $10.00/M = $5.00
        let cost = pricing
            .cost_for_usage("gpt-4o", 1_000_000, 500_000)
            .unwrap();
        assert!((cost - 7.50).abs() < 1e-9);

        let small = pricing.cost_for_usage("gpt-4o-mini", 100, 50).unwrap();
        // 100 * 0.15 / 1e6 = 1.5e-5; 50 * 0.60 / 1e6 = 3e-5; total 4.5e-5
        assert!((small - 4.5e-5).abs() < 1e-12);
    }

    #[test]
    fn pricing_config_unknown_model_returns_none() {
        let pricing = PricingConfig::new().with_price("gpt-4o", 2.50, 10.00);
        assert!(pricing.cost_for_usage("unknown-model", 100, 50).is_none());
    }

    #[test]
    fn pricing_config_empty_returns_none() {
        let pricing = PricingConfig::new();
        assert!(pricing.cost_for_usage("anything", 100, 50).is_none());
    }

    #[test]
    fn callback_context_fire_without_callback_is_noop() {
        let ctx = CallbackContext::default();
        // No callback installed => fire() returns without doing anything.
        // We can't construct a real CallbackInput cheaply here without panicking,
        // so we construct one with placeholder strings (the no-callback path
        // never reads them).
        let placeholder_request: MessageCreateRequest = serde_json::from_str(
            r#"{"model":"x","max_tokens":1,"messages":[{"role":"user","content":"a"}]}"#,
        )
        .unwrap();
        let placeholder_response: MessageResponse = serde_json::from_str(
            r#"{"id":"x","type":"message","role":"assistant","model":"x","content":[],"usage":{"input_tokens":0,"output_tokens":0}}"#,
        )
        .unwrap();
        ctx.fire(CallbackInput {
            request: placeholder_request,
            response: placeholder_response,
            request_model: String::new(),
            backend_model: String::new(),
            duration: Duration::ZERO,
            cost_usd: None,
        });
    }

    #[test]
    fn callback_context_catches_panic() {
        let cb: SuccessCallback = Arc::new(|_| panic!("intentional"));
        let ctx = CallbackContext {
            callback: Some(cb),
            ..Default::default()
        };
        // Should NOT panic.
        ctx.fire(CallbackInput {
            request: serde_json::from_str(
                r#"{"model":"x","max_tokens":1,"messages":[{"role":"user","content":"a"}]}"#,
            )
            .unwrap(),
            response: serde_json::from_str(
                r#"{"id":"x","type":"message","role":"assistant","model":"x","content":[],"usage":{"input_tokens":0,"output_tokens":0}}"#,
            )
            .unwrap(),
            request_model: "x".into(),
            backend_model: "x".into(),
            duration: Duration::ZERO,
            cost_usd: None,
        });
    }

    #[test]
    fn callback_context_dedups_pricing_warnings() {
        let pricing = PricingConfig::new().with_price("gpt-4o", 2.50, 10.00);
        let ctx = CallbackContext {
            pricing,
            ..Default::default()
        };
        // Same missing model id called many times — the warn-dedup HashSet
        // should not grow past 1 (we can't easily assert the log line in a
        // cargo test, but the HashSet count is observable).
        for _ in 0..100 {
            assert!(ctx.cost_for("missing-model", 100, 50).is_none());
        }
        assert_eq!(ctx.warned_missing_pricing.lock().unwrap().len(), 1);
    }
}
