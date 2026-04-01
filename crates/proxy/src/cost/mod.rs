// Model pricing loader and cost calculation.
//
// Loads pricing data from an embedded JSON file at startup. Calculates per-request
// cost from token counts by matching the backend model name against pricing entries.

pub mod db;

use dashmap::DashMap;
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
                    include_str!("../../../../assets/model_pricing.json").to_string()
                }
            }
        } else {
            include_str!("../../../../assets/model_pricing.json").to_string()
        };
        let entries: Vec<ModelPricingEntry> =
            serde_json::from_str(&json).expect("invalid model_pricing.json");
        Self { entries }
    }

    /// Return (input_cost_per_token, output_cost_per_token) for a model, or None if unknown.
    ///
    /// Same lookup order as cost_for_usage (exact then longest-prefix) but does not log
    /// on miss, so it is safe to call during routing decisions.
    pub fn price_for_model(&self, model: &str) -> Option<(f64, f64)> {
        if let Some(entry) = self.entries.iter().find(|e| e.model_pattern == model) {
            return Some((entry.input_cost_per_token, entry.output_cost_per_token));
        }
        let mut best: Option<&ModelPricingEntry> = None;
        let mut best_len: usize = 0;
        for entry in &self.entries {
            if model.starts_with(&entry.model_pattern) && entry.model_pattern.len() > best_len {
                best = Some(entry);
                best_len = entry.model_pattern.len();
            }
        }
        best.map(|e| (e.input_cost_per_token, e.output_cost_per_token))
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
    fn load_with_optional_override_uses_file() {
        use std::io::Write;
        let dir = std::env::temp_dir();
        let path = dir.join("test_model_pricing.json");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, r#"[{{"model_pattern":"test-only-model","input_cost_per_token":0.001,"output_cost_per_token":0.002,"provider":"test"}}]"#).unwrap();
        drop(f);
        let pricing = ModelPricing::load_with_optional_override(Some(path.to_str().unwrap()));
        std::fs::remove_file(&path).ok();
        assert_eq!(pricing.entries.len(), 1);
        assert_eq!(pricing.entries[0].model_pattern, "test-only-model");
        assert!((pricing.entries[0].input_cost_per_token - 0.001).abs() < 1e-10);
    }

    #[test]
    fn load_with_optional_override_none_uses_embedded() {
        let pricing = ModelPricing::load_with_optional_override(None);
        // Embedded pricing has many entries.
        assert!(
            pricing.entries.len() > 5,
            "embedded pricing should have multiple entries"
        );
    }

    #[test]
    fn load_with_optional_override_bad_path_falls_back_to_embedded() {
        let pricing =
            ModelPricing::load_with_optional_override(Some("/nonexistent/path/pricing.json"));
        assert!(
            pricing.entries.len() > 5,
            "bad path should fall back to embedded pricing"
        );
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
                allowed_models: None,
            },
        )
        .unwrap();

        let db = Arc::new(Mutex::new(conn));
        let (events_tx, _) = tokio::sync::broadcast::channel(1);
        let (log_tx, _) = tokio::sync::mpsc::channel(1);

        let shared = crate::admin::state::SharedState {
            db: db.clone(),
            events_tx,
            runtime_config: Arc::new(std::sync::RwLock::new(crate::admin::state::RuntimeConfig {
                model_mappings: indexmap::IndexMap::new(),
                log_level: "info".to_string(),
                log_bodies: false,
            })),
            backend_metrics: Arc::new(std::collections::HashMap::new()),
            log_tx,
            log_reload: None,
            config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            virtual_keys: Arc::new(dashmap::DashMap::new()),
            hmac_secret: Arc::new(b"test-secret".to_vec()),
            model_router: None,
            mcp_manager: None,
            issued_csrf_tokens: Arc::new(
                moka::sync::Cache::builder()
                    .max_capacity(1_000)
                    .time_to_live(std::time::Duration::from_secs(86400))
                    .build(),
            ),
        };

        let vk_ctx = VirtualKeyContext {
            key_id,
            rate_state: Arc::new(RateLimitState::new()),
            allowed_models: None,
            period_reset: None,
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

    // -- Spend threshold detection tests --

    #[test]
    fn spend_threshold_detection() {
        // Zero budget always returns 0 (no alerting).
        assert_eq!(spend_threshold_level(50.0, 0.0), 0);
        assert_eq!(spend_threshold_level(50.0, -10.0), 0);

        // Below 80%
        assert_eq!(spend_threshold_level(0.0, 100.0), 0);
        assert_eq!(spend_threshold_level(79.99, 100.0), 0);

        // At and above 80%
        assert_eq!(spend_threshold_level(80.0, 100.0), 1);
        assert_eq!(spend_threshold_level(85.0, 100.0), 1);
        assert_eq!(spend_threshold_level(94.99, 100.0), 1);

        // At and above 95%
        assert_eq!(spend_threshold_level(95.0, 100.0), 2);
        assert_eq!(spend_threshold_level(99.99, 100.0), 2);

        // At and above 100%
        assert_eq!(spend_threshold_level(100.0, 100.0), 3);
        assert_eq!(spend_threshold_level(150.0, 100.0), 3);
    }

    #[test]
    fn spend_threshold_below_80_returns_0() {
        // Boundary: 79.999...% is still below 80%.
        assert_eq!(spend_threshold_level(79.999, 100.0), 0);
        // Small budget, small spend.
        assert_eq!(spend_threshold_level(0.79, 1.0), 0);
        // Exactly at the boundary: 80/100 = 80%.
        assert_eq!(spend_threshold_level(0.80, 1.0), 1);
    }

    #[test]
    fn reset_alert_level_clears_map() {
        // Insert a tracked level.
        ALERT_LEVELS.insert(-999, 2);
        assert!(ALERT_LEVELS.contains_key(&-999));

        reset_alert_level(-999);
        assert!(!ALERT_LEVELS.contains_key(&-999));

        // Resetting a non-existent key is a no-op (should not panic).
        reset_alert_level(-998);
    }

    #[test]
    fn alert_dedup_fires_only_on_increase() {
        // Use a unique key_id to avoid collisions with other tests.
        let key_id = -1000;
        ALERT_LEVELS.remove(&key_id);

        // Simulate crossing 80% threshold.
        // maybe_fire_spend_alert is not easily testable for webhook firing
        // (no webhook configured in tests), but we can verify the dedup map.
        maybe_fire_spend_alert(key_id, "sk-vktest", 80.0, 100.0, Some("monthly"));
        assert_eq!(*ALERT_LEVELS.get(&key_id).unwrap(), 1);

        // Same level should not update (still 1).
        maybe_fire_spend_alert(key_id, "sk-vktest", 85.0, 100.0, Some("monthly"));
        assert_eq!(*ALERT_LEVELS.get(&key_id).unwrap(), 1);

        // Higher level (95%) should update.
        maybe_fire_spend_alert(key_id, "sk-vktest", 95.0, 100.0, Some("monthly"));
        assert_eq!(*ALERT_LEVELS.get(&key_id).unwrap(), 2);

        // 100% should update to 3.
        maybe_fire_spend_alert(key_id, "sk-vktest", 100.0, 100.0, Some("monthly"));
        assert_eq!(*ALERT_LEVELS.get(&key_id).unwrap(), 3);

        // Reset and verify re-alerting works.
        reset_alert_level(key_id);
        maybe_fire_spend_alert(key_id, "sk-vktest", 80.0, 100.0, Some("monthly"));
        assert_eq!(*ALERT_LEVELS.get(&key_id).unwrap(), 1);

        // Clean up.
        ALERT_LEVELS.remove(&key_id);
    }
}
