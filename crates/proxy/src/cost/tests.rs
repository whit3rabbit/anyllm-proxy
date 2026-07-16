use super::*;

fn test_pricing() -> ModelPricing {
    ModelPricing::from_entries(vec![
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
    ])
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
fn empty_pattern_is_not_a_catch_all() {
    // A custom pricing file with an empty model_pattern must not become a
    // catch-all that bills every unknown model; the miss path must still fire.
    let pricing = ModelPricing::from_entries(vec![ModelPricingEntry {
        model_pattern: String::new(),
        input_cost_per_token: 1.0,
        output_cost_per_token: 1.0,
        provider: "bogus".to_string(),
    }]);
    assert_eq!(pricing.price_for_model("anything"), None);
    assert_eq!(pricing.cost_for_usage("anything", 1000, 500), 0.0);
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
    let pricing = ModelPricing::load_with_optional_override(Some("/nonexistent/path/pricing.json"));
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
            allowed_routes: None,
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
            redact_secrets: false,
            anthropic_thinking_repair: false,
            pxpipe_compress: false,
            pxpipe_models: String::new(),
            rtk_compress: false,
            rtk_models: String::new(),
            forward_client_auth: false,
            tool_guardrail_mode: crate::tools::ToolGuardrailMode::Disabled
                .as_str()
                .to_string(),
            optimizer_mode: anyllm_optimize_core::Mode::Off.as_str().to_string(),
            router: Default::default(),
        })),
        runtime_defaults: crate::admin::state::RuntimeConfigDefaults {
            log_bodies: false,
            redact_secrets: false,
            anthropic_thinking_repair: false,
            pxpipe_compress: false,
            pxpipe_models: String::new(),
            rtk_compress: false,
            rtk_models: String::new(),
            forward_client_auth: false,
            tool_guardrail_mode: crate::tools::ToolGuardrailMode::Disabled
                .as_str()
                .to_string(),
            optimizer_mode: anyllm_optimize_core::Mode::Off.as_str().to_string(),
            router: Default::default(),
        },
        backend_metrics: Arc::new(std::collections::HashMap::new()),
        log_tx,
        log_reload: None,
        config_write_lock: Arc::new(tokio::sync::Mutex::new(())),
        virtual_keys: Arc::new(dashmap::DashMap::new()),
        hmac_secret: Arc::new(b"test-secret".to_vec()),
        model_router: None,
        route_router: None,
        provider_catalog: Arc::new(anyllm_providers::ProviderCatalog::bundled()),
        mcp_manager: None,
        issued_csrf_tokens: Arc::new(
            moka::sync::Cache::builder()
                .max_capacity(1_000)
                .time_to_live(std::time::Duration::from_secs(86400))
                .build(),
        ),
        started_at: std::time::SystemTime::now(),
        listen_port: 3000,
        managed_backends: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
        static_backends: Arc::new(std::collections::HashSet::new()),
    };

    let vk_ctx = VirtualKeyContext {
        key_id,
        rate_state: Arc::new(RateLimitState::new()),
        allowed_models: None,
        allowed_routes: None,
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
