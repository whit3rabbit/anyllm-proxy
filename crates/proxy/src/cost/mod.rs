// Model pricing loader and cost calculation.
//
// Loads pricing data from an embedded JSON file at startup. Calculates per-request
// cost from token counts by matching the backend model name against pricing entries.

pub mod db;

use std::sync::LazyLock;

/// Global pricing data, loaded once from embedded JSON at first access.
static PRICING: LazyLock<ModelPricing> = LazyLock::new(ModelPricing::load);

/// Access the global model pricing table.
pub fn pricing() -> &'static ModelPricing {
    &PRICING
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ModelPricingEntry {
    pub model_pattern: String,
    pub input_cost_per_token: f64,
    pub output_cost_per_token: f64,
    pub provider: String,
}

pub struct ModelPricing {
    entries: Vec<ModelPricingEntry>,
}

impl ModelPricing {
    /// Load from embedded JSON (compiled into the binary).
    pub fn load() -> Self {
        let json = include_str!("../../../../assets/model_pricing.json");
        let entries: Vec<ModelPricingEntry> =
            serde_json::from_str(json).expect("invalid model_pricing.json");
        Self { entries }
    }

    /// Calculate cost for a usage record.
    ///
    /// Matching strategy: exact match first, then longest prefix match.
    /// Returns 0.0 with a warning log if no match found.
    pub fn cost_for_usage(&self, model: &str, input_tokens: u64, output_tokens: u64) -> f64 {
        // 1. Try exact match
        if let Some(entry) = self.entries.iter().find(|e| e.model_pattern == model) {
            return entry.input_cost_per_token * input_tokens as f64
                + entry.output_cost_per_token * output_tokens as f64;
        }

        // 2. Try longest prefix match (e.g., "gpt-4o-2024-05-13" matches "gpt-4o")
        let mut best: Option<&ModelPricingEntry> = None;
        let mut best_len: usize = 0;
        for entry in &self.entries {
            if model.starts_with(&entry.model_pattern) && entry.model_pattern.len() > best_len {
                best = Some(entry);
                best_len = entry.model_pattern.len();
            }
        }

        if let Some(entry) = best {
            return entry.input_cost_per_token * input_tokens as f64
                + entry.output_cost_per_token * output_tokens as f64;
        }

        // 3. No match
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
        // Spawn a blocking task so the response is not delayed by the DB write.
        tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap_or_else(|e| e.into_inner());
            if let Err(e) = db::accumulate_spend(&conn, key_id, cost, input_tokens, output_tokens) {
                tracing::error!(error = %e, key_id, "failed to accumulate spend");
            }
        });
    }
    cost
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_pricing() -> ModelPricing {
        ModelPricing {
            entries: vec![
                ModelPricingEntry {
                    model_pattern: "gpt-4o".to_string(),
                    input_cost_per_token: 0.0000025,
                    output_cost_per_token: 0.00001,
                    provider: "openai".to_string(),
                },
                ModelPricingEntry {
                    model_pattern: "gpt-4o-mini".to_string(),
                    input_cost_per_token: 0.00000015,
                    output_cost_per_token: 0.0000006,
                    provider: "openai".to_string(),
                },
                ModelPricingEntry {
                    model_pattern: "gemini-2.5-pro".to_string(),
                    input_cost_per_token: 0.00000125,
                    output_cost_per_token: 0.00001,
                    provider: "google".to_string(),
                },
            ],
        }
    }

    #[test]
    fn exact_match() {
        let pricing = test_pricing();
        let cost = pricing.cost_for_usage("gpt-4o", 1000, 500);
        // 1000 * 0.0000025 + 500 * 0.00001 = 0.0025 + 0.005 = 0.0075
        let expected = 1000.0 * 0.0000025 + 500.0 * 0.00001;
        assert!((cost - expected).abs() < 1e-12);
    }

    #[test]
    fn exact_match_prefers_longer() {
        let pricing = test_pricing();
        // "gpt-4o-mini" should match the gpt-4o-mini entry, not gpt-4o
        let cost = pricing.cost_for_usage("gpt-4o-mini", 1000, 500);
        let expected = 1000.0 * 0.00000015 + 500.0 * 0.0000006;
        assert!((cost - expected).abs() < 1e-12);
    }

    #[test]
    fn prefix_match() {
        let pricing = test_pricing();
        // "gpt-4o-2024-05-13" should match "gpt-4o" by prefix
        let cost = pricing.cost_for_usage("gpt-4o-2024-05-13", 1000, 500);
        let expected = 1000.0 * 0.0000025 + 500.0 * 0.00001;
        assert!((cost - expected).abs() < 1e-12);
    }

    #[test]
    fn prefix_match_longest_wins() {
        let pricing = test_pricing();
        // "gpt-4o-mini-2024" should match "gpt-4o-mini" (longer prefix) not "gpt-4o"
        let cost = pricing.cost_for_usage("gpt-4o-mini-2024", 1000, 500);
        let expected = 1000.0 * 0.00000015 + 500.0 * 0.0000006;
        assert!((cost - expected).abs() < 1e-12);
    }

    #[test]
    fn unknown_model_returns_zero() {
        let pricing = test_pricing();
        let cost = pricing.cost_for_usage("totally-unknown-model", 1000, 500);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn zero_tokens() {
        let pricing = test_pricing();
        let cost = pricing.cost_for_usage("gpt-4o", 0, 0);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn load_embedded_pricing() {
        // Verify the embedded JSON parses without panic
        let pricing = ModelPricing::load();
        assert!(!pricing.entries.is_empty());
    }

    #[test]
    fn record_cost_without_shared_state_is_noop() {
        // When there is no shared state or virtual key context, record_cost
        // should return the computed cost but not attempt any DB write.
        let cost = record_cost(&None, &None, "gpt-4o", 1000, 500);
        // Should compute cost from global pricing (gpt-4o is in the embedded pricing).
        // Exact value depends on the embedded pricing data, but should be > 0.
        assert!(cost > 0.0);
    }

    #[test]
    fn record_cost_with_shared_state_persists_spend() {
        // Build a minimal SharedState with an in-memory SQLite DB to verify
        // that record_cost spawns a blocking task that writes to the DB.
        use crate::admin::db::{init_db, InsertVirtualKeyParams};
        use crate::admin::keys::RateLimitState;
        use crate::server::middleware::VirtualKeyContext;
        use std::sync::{Arc, Mutex};

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();

        let key_id = crate::admin::db::insert_virtual_key(
            &conn,
            &InsertVirtualKeyParams {
                key_hash: "0000000000000000000000000000000000000000000000000000000000000000",
                key_prefix: "sk-vktest",
                description: Some("cost test"),
                expires_at: None,
                rpm_limit: None,
                tpm_limit: None,
                spend_limit: None,
                role: "developer",
                max_budget_usd: Some(100.0),
                budget_duration: None,
            },
        )
        .unwrap();

        let db = Arc::new(Mutex::new(conn));
        let (events_tx, _) = tokio::sync::broadcast::channel(1);
        let (log_tx, _) = tokio::sync::mpsc::channel(1);

        let shared = crate::admin::state::SharedState {
            db: db.clone(),
            events_tx,
            runtime_config: Arc::new(std::sync::RwLock::new(
                crate::admin::state::RuntimeConfig {
                    model_mappings: indexmap::IndexMap::new(),
                    log_level: "info".to_string(),
                    log_bodies: false,
                },
            )),
            backend_metrics: Arc::new(std::collections::HashMap::new()),
            log_tx,
            log_reload: None,
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            virtual_keys: Arc::new(dashmap::DashMap::new()),
            model_router: None,
        };

        let vk_ctx = VirtualKeyContext {
            key_id,
            rate_state: Arc::new(RateLimitState::new()),
        };

        // record_cost uses tokio::task::spawn_blocking, so we need a runtime.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let cost = record_cost(&Some(shared), &Some(vk_ctx), "gpt-4o", 1000, 500);
            assert!(cost > 0.0);

            // Wait for the spawned blocking task to complete.
            tokio::task::yield_now().await;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        });

        // Verify the spend was persisted.
        let conn = db.lock().unwrap();
        let spend = db::get_key_spend(&conn, key_id).unwrap().unwrap();
        assert!(spend.total_cost_usd > 0.0);
        assert_eq!(spend.total_input_tokens, 1000);
        assert_eq!(spend.total_output_tokens, 500);
        assert_eq!(spend.request_count, 1);
    }
}
