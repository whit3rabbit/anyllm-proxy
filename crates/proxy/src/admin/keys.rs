// Virtual API key generation, hashing, and rate limit state.

use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Role assigned to a virtual API key, controlling access scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyRole {
    Admin,
    Developer,
}

impl KeyRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            KeyRole::Admin => "admin",
            KeyRole::Developer => "developer",
        }
    }

    pub fn from_str_or_default(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "admin" => KeyRole::Admin,
            _ => KeyRole::Developer,
        }
    }
}

/// Budget reset period for a virtual key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetDuration {
    Daily,
    Monthly,
}

impl BudgetDuration {
    pub fn as_str(&self) -> &'static str {
        match self {
            BudgetDuration::Daily => "daily",
            BudgetDuration::Monthly => "monthly",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "daily" => Some(BudgetDuration::Daily),
            "monthly" => Some(BudgetDuration::Monthly),
            _ => None,
        }
    }
}

/// Current time as milliseconds since the Unix epoch. Used for rate-limit sliding windows.
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Generate a new virtual API key.
/// Returns (raw_key, key_prefix, key_hash_hex).
/// The raw_key is shown once at creation; key_prefix is for display; key_hash_hex is stored.
pub fn generate_virtual_key() -> (String, String, String) {
    let a = uuid::Uuid::new_v4().as_simple().to_string();
    let b = uuid::Uuid::new_v4().as_simple().to_string();
    let raw_key = format!("sk-vk{}{}", a, b);
    let key_prefix = raw_key[..8].to_string();
    let key_hash_hex = hash_key(&raw_key);
    (raw_key, key_prefix, key_hash_hex)
}

/// SHA-256 hash a key string and return hex-encoded result.
pub fn hash_key(key: &str) -> String {
    let hash: [u8; 32] = Sha256::digest(key.as_bytes()).into();
    bytes_to_hex(&hash)
}

/// Convert a hex-encoded hash to raw bytes.
pub fn hash_from_hex(hex_str: &str) -> Option<[u8; 32]> {
    if hex_str.len() != 64 {
        return None;
    }
    let mut arr = [0u8; 32];
    for i in 0..32 {
        arr[i] = u8::from_str_radix(&hex_str[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(arr)
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// In-memory metadata for a virtual key (stored in DashMap).
#[derive(Debug)]
pub struct VirtualKeyMeta {
    pub id: i64,
    pub description: Option<String>,
    /// Epoch seconds; None = no expiry.
    pub expires_at: Option<i64>,
    pub rpm_limit: Option<u32>,
    pub tpm_limit: Option<u32>,
    pub rate_state: Arc<RateLimitState>,
    /// Access role (admin or developer). Defaults to developer.
    pub role: KeyRole,
    /// Maximum budget in USD per period. None = unlimited.
    pub max_budget_usd: Option<f64>,
    /// Budget reset period. None = lifetime budget (no reset).
    pub budget_duration: Option<BudgetDuration>,
    /// Start of the current budget period (ISO 8601 UTC).
    pub period_start: Option<String>,
    /// Accumulated spend in the current period.
    pub period_spend_usd: f64,
}

/// Sliding window rate limit state per virtual key.
#[derive(Debug)]
pub struct RateLimitState {
    pub rpm_window: Mutex<VecDeque<u64>>,
    pub tpm_window: Mutex<VecDeque<(u64, u32)>>,
}

impl Default for RateLimitState {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimitState {
    pub fn new() -> Self {
        Self {
            rpm_window: Mutex::new(VecDeque::new()),
            tpm_window: Mutex::new(VecDeque::new()),
        }
    }

    /// Check if a new request is within the RPM limit.
    /// Returns Ok(()) if allowed, Err(retry_after_secs) if exceeded.
    pub fn check_rpm(&self, limit: u32, now_ms: u64) -> Result<(), u64> {
        let mut window = self.rpm_window.lock().unwrap_or_else(|e| e.into_inner());
        let cutoff = now_ms.saturating_sub(60_000);
        // Drain expired entries
        while window.front().is_some_and(|&ts| ts < cutoff) {
            window.pop_front();
        }
        if window.len() >= limit as usize {
            // Compute retry-after: time until the oldest entry expires
            let oldest = window.front().copied().unwrap_or(now_ms);
            let retry_after_ms = (oldest + 60_000).saturating_sub(now_ms);
            return Err((retry_after_ms / 1000).max(1));
        }
        window.push_back(now_ms);
        Ok(())
    }

    /// Record a TPM token count for the current request.
    pub fn record_tpm(&self, now_ms: u64, tokens: u32) {
        let mut window = self.tpm_window.lock().unwrap_or_else(|e| e.into_inner());
        let cutoff = now_ms.saturating_sub(60_000);
        while window.front().is_some_and(|&(ts, _)| ts < cutoff) {
            window.pop_front();
        }
        window.push_back((now_ms, tokens));
    }

    /// Check if adding `tokens` would exceed the TPM limit.
    pub fn check_tpm(&self, limit: u32, now_ms: u64) -> Result<(), u64> {
        let mut window = self.tpm_window.lock().unwrap_or_else(|e| e.into_inner());
        let cutoff = now_ms.saturating_sub(60_000);
        while window.front().is_some_and(|&(ts, _)| ts < cutoff) {
            window.pop_front();
        }
        let total: u64 = window.iter().map(|&(_, t)| t as u64).sum();
        if total >= limit as u64 {
            let oldest = window.front().map(|&(ts, _)| ts).unwrap_or(now_ms);
            let retry_after_ms = (oldest + 60_000).saturating_sub(now_ms);
            return Err((retry_after_ms / 1000).max(1));
        }
        Ok(())
    }
}

/// Check whether the budget period has elapsed and reset spend if so.
/// Returns true if a reset occurred.
/// Does NOT persist to SQLite; caller should fire-and-forget a DB update.
pub fn check_and_reset_period(meta: &mut VirtualKeyMeta) -> bool {
    let duration = match meta.budget_duration {
        Some(d) => d,
        None => return false, // Lifetime budget, no periodic reset
    };

    let now_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let boundary_epoch = match &meta.period_start {
        Some(start) => next_period_boundary(start, duration),
        None => {
            // No period_start set yet; initialize it now
            meta.period_start = Some(current_period_start(now_epoch, duration));
            meta.period_spend_usd = 0.0;
            return true;
        }
    };

    if let Some(boundary) = boundary_epoch {
        if now_epoch >= boundary {
            meta.period_start = Some(current_period_start(now_epoch, duration));
            meta.period_spend_usd = 0.0;
            return true;
        }
    }
    false
}

/// Compute the epoch timestamp of the next period boundary given a period start ISO string.
fn next_period_boundary(start_iso: &str, duration: BudgetDuration) -> Option<u64> {
    // Parse the ISO 8601 date to extract year, month, day
    // Format: "2026-03-22T00:00:00Z"
    if start_iso.len() < 10 {
        return None;
    }
    let year: u64 = start_iso[0..4].parse().ok()?;
    let month: u64 = start_iso[5..7].parse().ok()?;
    let day: u64 = start_iso[8..10].parse().ok()?;

    match duration {
        BudgetDuration::Daily => {
            // Next day at UTC midnight
            let start_epoch = ymd_to_epoch(year, month, day);
            Some(start_epoch + 86400)
        }
        BudgetDuration::Monthly => {
            // 1st of next month at UTC midnight
            let (ny, nm) = if month == 12 {
                (year + 1, 1)
            } else {
                (year, month + 1)
            };
            Some(ymd_to_epoch(ny, nm, 1))
        }
    }
}

/// Compute the current period start for a given epoch time.
fn current_period_start(now_epoch: u64, duration: BudgetDuration) -> String {
    let days = now_epoch / 86400;
    let (year, month, day) = super::db::days_to_ymd(days);
    match duration {
        BudgetDuration::Daily => {
            format!("{year:04}-{month:02}-{day:02}T00:00:00Z")
        }
        BudgetDuration::Monthly => {
            format!("{year:04}-{month:02}-01T00:00:00Z")
        }
    }
}

/// Convert year/month/day to epoch seconds (UTC midnight).
fn ymd_to_epoch(year: u64, month: u64, day: u64) -> u64 {
    // Inverse of the Hinnant algorithm used in db.rs
    let y = if month <= 2 { year - 1 } else { year };
    let m = if month <= 2 { month + 9 } else { month - 3 };
    let era = y / 400;
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    days * 86400
}

/// Compute the ISO 8601 string for when the current period resets.
pub fn period_reset_at(meta: &VirtualKeyMeta) -> Option<String> {
    let duration = meta.budget_duration?;
    let start = meta.period_start.as_ref()?;
    let boundary = next_period_boundary(start, duration)?;
    Some(super::db::epoch_to_iso8601(boundary))
}

/// Row from the virtual_api_key table.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VirtualKeyRow {
    pub id: i64,
    pub key_hash: String,
    pub key_prefix: String,
    pub description: Option<String>,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub revoked_at: Option<String>,
    pub rpm_limit: Option<u32>,
    pub tpm_limit: Option<u32>,
    pub spend_limit: Option<f64>,
    pub total_spend: f64,
    pub total_requests: i64,
    pub total_tokens: i64,
    pub role: String,
    pub max_budget_usd: Option<f64>,
    pub budget_duration: Option<String>,
    pub period_start: Option<String>,
    pub period_spend_usd: f64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
}

impl VirtualKeyRow {
    /// Compute the effective status of a key.
    pub fn status(&self) -> &'static str {
        if self.revoked_at.is_some() {
            return "revoked";
        }
        if let Some(ref exp) = self.expires_at {
            if *exp <= super::db::now_iso8601() {
                return "expired";
            }
        }
        "active"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_generation_format() {
        let (raw, prefix, hash) = generate_virtual_key();
        assert!(raw.starts_with("sk-vk"));
        assert_eq!(prefix.len(), 8);
        assert!(prefix.starts_with("sk-vk"));
        assert_eq!(hash.len(), 64); // hex SHA-256
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
        };
        // No reset because no duration
        assert!(!check_and_reset_period(&mut meta));
        assert_eq!(meta.period_spend_usd, 5.0);
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
        };
        // Period start is in 2020, so it should reset
        assert!(check_and_reset_period(&mut meta));
        assert_eq!(meta.period_spend_usd, 0.0);
        assert!(meta.period_start.is_some());
    }
}
