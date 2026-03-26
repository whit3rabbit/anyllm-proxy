// Virtual API key generation, hashing, and rate limit state.

use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

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
}
