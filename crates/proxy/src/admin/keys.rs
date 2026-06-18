// Virtual API key generation, hashing, and rate limit state.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

type HmacSha256 = Hmac<Sha256>;

/// Role assigned to a virtual API key, controlling access scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyRole {
    Admin,
    Developer,
}

impl KeyRole {
    /// Return the lowercase string label stored in SQLite and returned by the admin API.
    pub fn as_str(&self) -> &'static str {
        match self {
            KeyRole::Admin => "admin",
            KeyRole::Developer => "developer",
        }
    }

    /// Parse a role string. Any unrecognised value is treated as `Developer` for safety.
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
    /// Return the lowercase string label stored in SQLite and returned by the admin API.
    pub fn as_str(&self) -> &'static str {
        match self {
            BudgetDuration::Daily => "daily",
            BudgetDuration::Monthly => "monthly",
        }
    }

    /// Parse a duration string. Returns `None` for unrecognised values.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "daily" => Some(BudgetDuration::Daily),
            "monthly" => Some(BudgetDuration::Monthly),
            _ => None,
        }
    }
}

/// Current time as milliseconds since the Unix epoch. Used for rate-limit sliding windows.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Generate a new virtual API key, hashed with HMAC-SHA256 using the installation secret.
/// Returns (raw_key, key_prefix, key_hash_hex).
/// The raw_key is shown once at creation; key_prefix is for display; key_hash_hex is stored.
pub fn generate_virtual_key(hmac_secret: &[u8]) -> (String, String, String) {
    let a = uuid::Uuid::new_v4().as_simple().to_string();
    let b = uuid::Uuid::new_v4().as_simple().to_string();
    let raw_key = format!("sk-vk{}{}", a, b);
    let key_prefix = raw_key[..8].to_string();
    let key_hash_hex = hmac_hash_key(&raw_key, hmac_secret);
    (raw_key, key_prefix, key_hash_hex)
}

/// SHA-256 hash a key string and return hex-encoded result.
/// Used for legacy keys created before HMAC was introduced.
pub fn hash_key(key: &str) -> String {
    let hash: [u8; 32] = Sha256::digest(key.as_bytes()).into();
    bytes_to_hex(&hash)
}

/// HMAC-SHA256 hash a key with a per-installation secret. Returns hex string.
/// Used for all newly created keys. The secret binds hashes to this installation,
/// so a stolen database cannot be used to brute-force keys on a different instance.
pub fn hmac_hash_key(key: &str, secret: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC-SHA256 accepts any key length");
    mac.update(key.as_bytes());
    let result = mac.finalize();
    bytes_to_hex(&result.into_bytes())
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
    /// Optional model allowlist. None = all models allowed.
    /// Supports exact match and prefix wildcard (e.g., `"claude-*"`).
    pub allowed_models: Option<Vec<String>>,
    /// Optional route allowlist. None = all routes allowed.
    pub allowed_routes: Option<Vec<String>>,
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
    /// Create an empty rate limit state. VecDeque provides O(1) front-pop
    /// for the sliding window expiry drain without reallocating.
    pub fn new() -> Self {
        Self {
            rpm_window: Mutex::new(VecDeque::new()),
            tpm_window: Mutex::new(VecDeque::new()),
        }
    }

    /// Check if a new request is within the RPM limit.
    /// Returns `Ok(())` if allowed, `Err(retry_after_secs)` if exceeded.
    ///
    /// Implements a 60-second sliding window: timestamps older than 60 s are
    /// evicted before checking. `>= limit` (not `>`) ensures the limit is a
    /// strict ceiling — a limit of 10 RPM allows exactly 10 requests per window.
    /// The `retry_after` value is rounded up to at least 1 s (HTTP Retry-After
    /// is in whole seconds; 0 would mislead clients into retrying immediately).
    pub fn check_rpm(&self, limit: u32, now_ms: u64) -> Result<(), u64> {
        let mut window = self.rpm_window.lock().unwrap_or_else(|e| e.into_inner());
        // 60_000 ms = 1 minute; the window size for RPM limiting.
        let cutoff = now_ms.saturating_sub(60_000);
        // Drain expired entries from the front (VecDeque is ordered oldest-first).
        while window.front().is_some_and(|&ts| ts < cutoff) {
            window.pop_front();
        }
        if window.len() >= limit as usize {
            // Time until the oldest entry ages out of the 60-second window.
            let oldest = window.front().copied().unwrap_or(now_ms);
            let retry_after_ms = (oldest + 60_000).saturating_sub(now_ms);
            // Divide ms -> s and clamp to 1 s minimum (HTTP Retry-After is whole seconds).
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
            // Lazy init: period_start is None until the first spend event. Initializing
            // here (rather than at key creation) means budget windows align to actual
            // usage, not creation time, which is more intuitive for low-traffic keys.
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
///
/// Avoids pulling in `chrono` or `time` crates by doing the arithmetic directly with
/// the Hinnant algorithm (same as `db::days_to_ymd`). This keeps the crate dependency
/// footprint small for a function that is called on every authenticated request.
fn next_period_boundary(start_iso: &str, duration: BudgetDuration) -> Option<u64> {
    // Parse the ISO 8601 date to extract year, month, day.
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

/// Compute `period_reset_at` from a database row where `budget_duration`
/// is stored as a string ("daily" or "monthly").
pub fn period_reset_at_from_row(row: &VirtualKeyRow) -> Option<String> {
    let duration = match row.budget_duration.as_deref()? {
        "daily" => BudgetDuration::Daily,
        "monthly" => BudgetDuration::Monthly,
        _ => return None,
    };
    let start = row.period_start.as_ref()?;
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
    pub allowed_models: Option<Vec<String>>,
    /// Optional route allowlist. None = all routes allowed.
    pub allowed_routes: Option<Vec<String>>,
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
mod tests;
