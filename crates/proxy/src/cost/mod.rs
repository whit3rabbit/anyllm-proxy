// Model pricing loader and cost calculation.
//
// Loads pricing data from an embedded JSON file at startup. Calculates per-request
// cost from token counts by matching the backend model name against pricing entries.

pub mod db;

use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::LazyLock;

/// Global pricing data, loaded once from embedded JSON at first access.
static PRICING: LazyLock<ModelPricing> = LazyLock::new(ModelPricing::load);

/// Tracks the highest alert level sent per key to avoid duplicate alerts.
/// Key: virtual key DB id, Value: highest threshold level (0-3).
static ALERT_LEVELS: LazyLock<DashMap<i64, u8>> = LazyLock::new(DashMap::new);

/// Returns the spend alert level: 0=none, 1=80%, 2=95%, 3=100%.
pub fn spend_threshold_level(spend: f64, budget: f64) -> u8 {
    if budget <= 0.0 {
        return 0;
    }
    let pct = spend / budget * 100.0;
    if pct >= 100.0 {
        3
    } else if pct >= 95.0 {
        2
    } else if pct >= 80.0 {
        1
    } else {
        0
    }
}

/// Reset alert tracking for a key (call on budget period rollover).
pub fn reset_alert_level(key_id: i64) {
    ALERT_LEVELS.remove(&key_id);
}

/// Check whether a spend alert should fire and, if so, send it via webhooks.
///
/// Only fires when the threshold level increases (dedup). The webhook payload
/// includes key metadata and the crossed threshold percentage.
fn maybe_fire_spend_alert(
    key_id: i64,
    key_prefix: &str,
    period_spend_usd: f64,
    max_budget_usd: f64,
    budget_duration: Option<&str>,
) {
    let level = spend_threshold_level(period_spend_usd, max_budget_usd);
    if level == 0 {
        return;
    }

    // Check and update dedup map atomically.
    let should_fire = {
        let mut entry = ALERT_LEVELS.entry(key_id).or_insert(0);
        if level > *entry {
            *entry = level;
            true
        } else {
            false
        }
    };

    if !should_fire {
        return;
    }

    let threshold_pct: u8 = match level {
        1 => 80,
        2 => 95,
        _ => 100,
    };

    tracing::warn!(
        key_id,
        key_prefix,
        threshold_pct,
        period_spend_usd,
        max_budget_usd,
        "spend threshold crossed"
    );

    // Fire webhook if configured (uses the global OnceLock from routes).
    let payload = serde_json::json!({
        "type": "spend_alert",
        "key_id": key_id,
        "key_prefix": key_prefix,
        "threshold_pct": threshold_pct,
        "period_spend_usd": period_spend_usd,
        "max_budget_usd": max_budget_usd,
        "budget_duration": budget_duration.unwrap_or("lifetime"),
    });

    if let Some(cb) = crate::server::routes::get_callbacks() {
        cb.notify_json(&payload);
    }
}

/// Access the global model pricing table.
pub fn pricing() -> &'static ModelPricing {
    &PRICING
}

/// Return (input_per_million, output_per_million) for a model, or None if unknown.
/// Costs are scaled to per-million for human-readable display; the underlying
/// table stores per-token values.
pub fn price_per_million_for_model(model_id: &str) -> Option<(f64, f64)> {
    pricing()
        .price_for_model(model_id)
        .map(|(i, o)| (i * 1_000_000.0, o * 1_000_000.0))
}

/// A single pricing record loaded from the JSON pricing table.
/// `model_pattern` supports exact match and longest-prefix matching.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ModelPricingEntry {
    pub model_pattern: String,
    pub input_cost_per_token: f64,
    pub output_cost_per_token: f64,
    pub provider: String,
}

/// The full model pricing table. Loaded at startup from embedded JSON or `MODEL_PRICING_FILE`.
pub struct ModelPricing {
    entries: Vec<ModelPricingEntry>,
    exact_index: HashMap<String, usize>,
    prefix_indexes: Vec<usize>,
}

impl ModelPricing {
    /// Load pricing from embedded JSON, or from the file at `MODEL_PRICING_FILE` if set.
    pub fn load() -> Self {
        let override_path = std::env::var("MODEL_PRICING_FILE").ok();
        Self::load_with_optional_override(override_path.as_deref())
    }

    /// Load pricing from `path` if provided and readable, otherwise fall back to embedded JSON.
    pub fn load_with_optional_override(path: Option<&str>) -> Self {
        let json = if let Some(p) = path {
            match std::fs::read_to_string(p) {
                Ok(content) => {
                    tracing::info!(path = %p, "loaded model pricing from MODEL_PRICING_FILE");
                    content
                }
                Err(e) => {
                    tracing::error!(
                        path = %p,
                        error = %e,
                        "failed to read MODEL_PRICING_FILE; falling back to embedded pricing"
                    );
                    include_str!("../../assets/model_pricing.json").to_string()
                }
            }
        } else {
            include_str!("../../assets/model_pricing.json").to_string()
        };
        let entries: Vec<ModelPricingEntry> =
            serde_json::from_str(&json).expect("invalid model_pricing.json");
        Self::from_entries(entries)
    }

    fn from_entries(entries: Vec<ModelPricingEntry>) -> Self {
        let mut exact_index = HashMap::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            exact_index
                .entry(entry.model_pattern.clone())
                .or_insert(index);
        }

        let mut prefix_indexes: Vec<usize> = (0..entries.len()).collect();
        prefix_indexes.sort_by(|left, right| {
            entries[*right]
                .model_pattern
                .len()
                .cmp(&entries[*left].model_pattern.len())
                .then_with(|| left.cmp(right))
        });

        Self {
            entries,
            exact_index,
            prefix_indexes,
        }
    }

    fn entry_for_model(&self, model: &str) -> Option<&ModelPricingEntry> {
        if let Some(index) = self.exact_index.get(model) {
            return self.entries.get(*index);
        }

        // Skip empty patterns: `model.starts_with("")` is always true, which would
        // turn an empty model_pattern (possible in a custom MODEL_PRICING_FILE) into
        // a silent catch-all and bill every unknown model at that rate instead of
        // logging the billing-leak miss.
        self.prefix_indexes
            .iter()
            .map(|index| &self.entries[*index])
            .find(|entry| {
                !entry.model_pattern.is_empty() && model.starts_with(&entry.model_pattern)
            })
    }

    /// Return (input_cost_per_token, output_cost_per_token) for a model, or None if unknown.
    ///
    /// Same lookup order as cost_for_usage (exact then longest-prefix) but does not log
    /// on miss, so it is safe to call during routing decisions.
    pub fn price_for_model(&self, model: &str) -> Option<(f64, f64)> {
        self.entry_for_model(model)
            .map(|entry| (entry.input_cost_per_token, entry.output_cost_per_token))
    }

    /// Calculate cost for a usage record.
    ///
    /// Matching strategy: exact match first, then longest prefix match.
    /// Returns 0.0 with a warning log if no match found.
    pub fn cost_for_usage(&self, model: &str, input_tokens: u64, output_tokens: u64) -> f64 {
        if let Some(entry) = self.entry_for_model(model) {
            return entry.input_cost_per_token * input_tokens as f64
                + entry.output_cost_per_token * output_tokens as f64;
        }

        tracing::error!(
            model = model,
            "BILLING LEAK: no pricing entry found for model, cost set to 0.0"
        );
        0.0
    }
}

/// Record cost for a completed request against a virtual key.
///
/// Calculates cost from token usage and the resolved model name, then
/// persists the spend to SQLite asynchronously. Returns the computed cost
/// so the caller can set the `x-anyllm-cost-usd` header.
pub fn record_cost(
    shared: &Option<crate::admin::state::SharedState>,
    vk_ctx: &Option<crate::server::middleware::VirtualKeyContext>,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
) -> f64 {
    let cost = pricing().cost_for_usage(model, input_tokens, output_tokens);
    if cost <= 0.0 {
        return cost;
    }
    if let (Some(shared), Some(ctx)) = (shared, vk_ctx) {
        let db = shared.db.clone();
        let key_id = ctx.key_id;
        let period_reset = ctx.period_reset.clone();
        // Spawn a blocking task so the response is not delayed by the DB write.
        tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap_or_else(|e| e.into_inner());
            // If the budget period rolled over during auth, reset SQLite first so that
            // accumulate_spend starts from 0 instead of adding to the stale old-period total.
            if let Some(ref new_period_start) = period_reset {
                if let Err(e) = db::reset_period_spend(&conn, key_id, new_period_start) {
                    tracing::error!(error = %e, key_id, "failed to reset period spend");
                }
                reset_alert_level(key_id);
            }
            if let Err(e) = db::accumulate_spend(&conn, key_id, cost, input_tokens, output_tokens) {
                tracing::error!(error = %e, key_id, "failed to accumulate spend");
                return;
            }
            // Check spend thresholds after accumulation.
            if let Ok(Some(spend)) = db::get_key_spend(&conn, key_id) {
                if let Some(budget) = spend.max_budget_usd {
                    maybe_fire_spend_alert(
                        key_id,
                        &spend.key_prefix,
                        spend.period_cost_usd,
                        budget,
                        spend.budget_duration.as_deref(),
                    );
                }
            }
        });
    }
    cost
}

#[cfg(test)]
mod tests;
