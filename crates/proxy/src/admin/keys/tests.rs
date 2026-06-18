use super::*;

#[test]
fn key_generation_format() {
    let secret = b"test-secret";
    let (raw, prefix, hash) = generate_virtual_key(secret);
    assert!(raw.starts_with("sk-vk"));
    assert_eq!(prefix.len(), 8);
    assert!(prefix.starts_with("sk-vk"));
    assert_eq!(hash.len(), 64); // hex HMAC-SHA256
}

#[test]
fn hash_deterministic() {
    let h1 = hash_key("test-key-123");
    let h2 = hash_key("test-key-123");
    assert_eq!(h1, h2);
}

#[test]
fn hash_from_hex_roundtrip() {
    let hex = hash_key("test");
    let bytes = hash_from_hex(&hex).unwrap();
    assert_eq!(bytes_to_hex(&bytes), hex);
}

#[test]
fn rpm_within_limit() {
    let state = RateLimitState::new();
    let now = 1000000;
    assert!(state.check_rpm(3, now).is_ok());
    assert!(state.check_rpm(3, now + 1).is_ok());
    assert!(state.check_rpm(3, now + 2).is_ok());
    // 4th request should be rejected
    assert!(state.check_rpm(3, now + 3).is_err());
}

#[test]
fn rpm_window_expiry() {
    let state = RateLimitState::new();
    let now = 1000000;
    assert!(state.check_rpm(1, now).is_ok());
    assert!(state.check_rpm(1, now + 100).is_err());
    // After 60 seconds, window should clear
    assert!(state.check_rpm(1, now + 60_001).is_ok());
}

#[test]
fn tpm_within_limit() {
    let state = RateLimitState::new();
    let now = 1000000;
    state.record_tpm(now, 50);
    assert!(state.check_tpm(100, now + 1).is_ok());
    state.record_tpm(now + 1, 50);
    // At limit
    assert!(state.check_tpm(100, now + 2).is_err());
}

#[test]
fn tpm_window_expiry() {
    let state = RateLimitState::new();
    let now = 1000000;
    state.record_tpm(now, 100);
    assert!(state.check_tpm(100, now + 1).is_err());
    // After 60 seconds
    assert!(state.check_tpm(100, now + 60_001).is_ok());
}

// -- KeyRole tests --

#[test]
fn key_role_roundtrip() {
    assert_eq!(KeyRole::Admin.as_str(), "admin");
    assert_eq!(KeyRole::Developer.as_str(), "developer");
    assert_eq!(KeyRole::from_str_or_default("admin"), KeyRole::Admin);
    assert_eq!(KeyRole::from_str_or_default("Admin"), KeyRole::Admin);
    assert_eq!(
        KeyRole::from_str_or_default("developer"),
        KeyRole::Developer
    );
    assert_eq!(KeyRole::from_str_or_default("unknown"), KeyRole::Developer);
    assert_eq!(KeyRole::from_str_or_default(""), KeyRole::Developer);
}

// -- BudgetDuration tests --

#[test]
fn budget_duration_roundtrip() {
    assert_eq!(BudgetDuration::Daily.as_str(), "daily");
    assert_eq!(BudgetDuration::Monthly.as_str(), "monthly");
    assert_eq!(BudgetDuration::parse("daily"), Some(BudgetDuration::Daily));
    assert_eq!(
        BudgetDuration::parse("Monthly"),
        Some(BudgetDuration::Monthly)
    );
    assert_eq!(BudgetDuration::parse("weekly"), None);
}

// -- Period boundary tests --

#[test]
fn ymd_to_epoch_known_values() {
    // 1970-01-01 = epoch 0
    assert_eq!(ymd_to_epoch(1970, 1, 1), 0);
    // 2020-01-01 = 1577836800
    assert_eq!(ymd_to_epoch(2020, 1, 1), 1577836800);
}

#[test]
fn next_period_boundary_daily() {
    let start = "2026-03-25T00:00:00Z";
    let boundary = next_period_boundary(start, BudgetDuration::Daily).unwrap();
    // Should be 2026-03-26 midnight
    let expected = ymd_to_epoch(2026, 3, 26);
    assert_eq!(boundary, expected);
}

#[test]
fn next_period_boundary_monthly() {
    let start = "2026-03-01T00:00:00Z";
    let boundary = next_period_boundary(start, BudgetDuration::Monthly).unwrap();
    // Should be 2026-04-01 midnight
    let expected = ymd_to_epoch(2026, 4, 1);
    assert_eq!(boundary, expected);
}

#[test]
fn next_period_boundary_monthly_december() {
    let start = "2026-12-01T00:00:00Z";
    let boundary = next_period_boundary(start, BudgetDuration::Monthly).unwrap();
    // Should be 2027-01-01 midnight
    let expected = ymd_to_epoch(2027, 1, 1);
    assert_eq!(boundary, expected);
}

#[test]
fn check_and_reset_period_no_duration() {
    let mut meta = VirtualKeyMeta {
        id: 1,
        description: None,
        expires_at: None,
        rpm_limit: None,
        tpm_limit: None,
        rate_state: Arc::new(RateLimitState::new()),
        role: KeyRole::Developer,
        max_budget_usd: Some(10.0),
        budget_duration: None, // lifetime, no reset
        period_start: Some("2020-01-01T00:00:00Z".to_string()),
        period_spend_usd: 5.0,
        allowed_models: None,
        allowed_routes: None,
    };
    // No reset because no duration
    assert!(!check_and_reset_period(&mut meta));
    assert_eq!(meta.period_spend_usd, 5.0);
}

#[test]
fn hmac_hash_differs_from_plain_sha256() {
    let key = "sk-vktest1234";
    let secret = b"install-secret-abc";
    let hmac_hash = hmac_hash_key(key, secret);
    let plain_hash = hash_key(key);
    assert_ne!(hmac_hash, plain_hash);
}

#[test]
fn hmac_hash_differs_with_different_secrets() {
    let key = "sk-vktest1234";
    let h1 = hmac_hash_key(key, b"secret-a");
    let h2 = hmac_hash_key(key, b"secret-b");
    assert_ne!(h1, h2);
}

#[test]
fn hmac_hash_deterministic() {
    let key = "sk-vktest1234";
    let secret = b"consistent-secret";
    assert_eq!(hmac_hash_key(key, secret), hmac_hash_key(key, secret));
}

#[test]
fn check_and_reset_period_resets_when_past_boundary() {
    let mut meta = VirtualKeyMeta {
        id: 1,
        description: None,
        expires_at: None,
        rpm_limit: None,
        tpm_limit: None,
        rate_state: Arc::new(RateLimitState::new()),
        role: KeyRole::Developer,
        max_budget_usd: Some(10.0),
        budget_duration: Some(BudgetDuration::Daily),
        period_start: Some("2020-01-01T00:00:00Z".to_string()),
        period_spend_usd: 5.0,
        allowed_models: None,
        allowed_routes: None,
    };
    // Period start is in 2020, so it should reset
    assert!(check_and_reset_period(&mut meta));
    assert_eq!(meta.period_spend_usd, 0.0);
    assert!(meta.period_start.is_some());
}

#[test]
fn period_reset_at_from_row_daily() {
    let row = VirtualKeyRow {
        id: 1,
        key_hash: String::new(),
        key_prefix: "sk-vk1234".into(),
        description: None,
        created_at: "2026-04-01T00:00:00Z".into(),
        expires_at: None,
        revoked_at: None,
        rpm_limit: None,
        tpm_limit: None,
        spend_limit: None,
        total_spend: 0.0,
        total_requests: 0,
        total_tokens: 0,
        role: "developer".into(),
        max_budget_usd: Some(10.0),
        budget_duration: Some("daily".into()),
        period_start: Some("2026-04-04T00:00:00Z".into()),
        period_spend_usd: 0.0,
        total_input_tokens: 0,
        total_output_tokens: 0,
        allowed_models: None,
        allowed_routes: None,
    };
    let reset = period_reset_at_from_row(&row);
    assert_eq!(reset, Some("2026-04-05T00:00:00Z".to_string()));
}
