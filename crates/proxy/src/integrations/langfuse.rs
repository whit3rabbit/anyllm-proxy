// Langfuse named callback integration.
//
// Sends LLM generation events to Langfuse's batch ingestion API.
// Activated when LANGFUSE_PUBLIC_KEY and LANGFUSE_SECRET_KEY are set,
// either explicitly via env vars or when "langfuse" appears in
// litellm_settings.callbacks.
//
// Fire-and-forget: send() spawns a tokio task and returns immediately.

use crate::admin::state::RequestLogEntry;
use std::sync::Arc;

pub struct LangfuseClient {
    pub(crate) public_key: String,
    pub(crate) secret_key: String,
    pub(crate) host: String,
    client: reqwest::Client,
}

impl LangfuseClient {
    /// Construct from env vars: LANGFUSE_PUBLIC_KEY, LANGFUSE_SECRET_KEY, LANGFUSE_HOST.
    /// Returns None when either required key is absent or empty.
    pub fn from_env() -> Option<Arc<Self>> {
        let public_key = std::env::var("LANGFUSE_PUBLIC_KEY").ok()?;
        let secret_key = std::env::var("LANGFUSE_SECRET_KEY").ok()?;
        if public_key.is_empty() || secret_key.is_empty() {
            return None;
        }
        let host = std::env::var("LANGFUSE_HOST")
            .unwrap_or_else(|_| "https://cloud.langfuse.com".to_string());
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("langfuse http client");
        Some(Arc::new(Self {
            public_key,
            secret_key,
            host,
            client,
        }))
    }

    /// Fire-and-forget: POST generation event to Langfuse ingestion API.
    /// Returns immediately; the HTTP call happens in a spawned tokio task.
    pub fn send(&self, entry: &RequestLogEntry) {
        let payload = build_generation_payload(entry);
        let url = format!("{}/api/public/ingestion", self.host.trim_end_matches('/'));
        let auth = format!(
            "Basic {}",
            base64_encode(format!("{}:{}", self.public_key, self.secret_key).as_bytes())
        );
        let client = self.client.clone();
        tokio::spawn(async move {
            match client
                .post(&url)
                .header("Authorization", &auth)
                .json(&payload)
                .send()
                .await
            {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        tracing::debug!(
                            url = %url,
                            status = %resp.status(),
                            "langfuse ingestion returned non-2xx"
                        );
                    }
                }
                Err(e) => {
                    tracing::debug!(error = %e, "langfuse ingestion request failed");
                }
            }
        });
    }
}

/// Build the Langfuse batch ingestion payload for a single generation.
pub(crate) fn build_generation_payload(entry: &RequestLogEntry) -> serde_json::Value {
    let end_time = &entry.timestamp;
    let start_time = iso8601_to_epoch_ms(end_time)
        .map(|end_ms| {
            let start_ms = end_ms.saturating_sub(entry.latency_ms);
            crate::admin::db::epoch_to_iso8601_ms(start_ms)
        })
        .unwrap_or_else(|| end_time.clone());

    let model = entry
        .model_mapped
        .as_deref()
        .or(entry.model_requested.as_deref())
        .unwrap_or("unknown");

    let mut usage = serde_json::json!({ "unit": "TOKENS" });
    if let Some(input) = entry.input_tokens {
        usage["input"] = serde_json::json!(input);
    }
    if let Some(output) = entry.output_tokens {
        usage["output"] = serde_json::json!(output);
    }
    if let Some(cost) = entry.cost_usd {
        usage["totalCost"] = serde_json::json!(cost);
    }

    let level = if entry.error_message.is_some() { "ERROR" } else { "DEFAULT" };

    let mut metadata = serde_json::json!({
        "backend": entry.backend,
        "status_code": entry.status_code,
        "latency_ms": entry.latency_ms,
    });
    if let Some(ref msg) = entry.error_message {
        metadata["error"] = serde_json::json!(msg);
    }

    serde_json::json!({
        "batch": [{
            "id": entry.request_id,
            "type": "generation-create",
            "timestamp": end_time,
            "body": {
                "id": entry.request_id,
                "name": model,
                "startTime": start_time,
                "endTime": end_time,
                "model": model,
                "usage": usage,
                "level": level,
                "metadata": metadata,
            }
        }]
    })
}

/// Encode bytes as standard base64 (RFC 4648). No external dependencies.
pub(crate) fn base64_encode(input: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 2 < input.len() {
        let b = ((input[i] as u32) << 16)
            | ((input[i + 1] as u32) << 8)
            | (input[i + 2] as u32);
        out.push(CHARS[((b >> 18) & 0x3f) as usize] as char);
        out.push(CHARS[((b >> 12) & 0x3f) as usize] as char);
        out.push(CHARS[((b >> 6) & 0x3f) as usize] as char);
        out.push(CHARS[(b & 0x3f) as usize] as char);
        i += 3;
    }
    let rem = input.len() - i;
    if rem == 1 {
        let b = (input[i] as u32) << 16;
        out.push(CHARS[((b >> 18) & 0x3f) as usize] as char);
        out.push(CHARS[((b >> 12) & 0x3f) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let b = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
        out.push(CHARS[((b >> 18) & 0x3f) as usize] as char);
        out.push(CHARS[((b >> 12) & 0x3f) as usize] as char);
        out.push(CHARS[((b >> 6) & 0x3f) as usize] as char);
        out.push('=');
    }
    out
}

/// Parse ISO 8601 UTC timestamp ("2026-03-27T10:15:30Z") to Unix epoch seconds.
/// Returns None on parse failure.
pub fn iso8601_to_epoch(s: &str) -> Option<u64> {
    if s.len() < 20 {
        return None;
    }
    let year: i64 = s[0..4].parse().ok()?;
    let month: i64 = s[5..7].parse().ok()?;
    let day: i64 = s[8..10].parse().ok()?;
    let hour: i64 = s[11..13].parse().ok()?;
    let min: i64 = s[14..16].parse().ok()?;
    let sec: i64 = s[17..19].parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let days = days_from_civil(year, month, day);
    let total = days * 86400 + hour * 3600 + min * 60 + sec;
    u64::try_from(total).ok()
}

/// Parse ISO 8601 UTC timestamp to Unix epoch milliseconds, retaining sub-second precision.
/// Handles "2026-03-27T10:15:30Z" (returns seconds * 1000) and
/// "2026-03-27T10:15:30.750Z" (retains the fractional ms component).
pub(crate) fn iso8601_to_epoch_ms(s: &str) -> Option<u64> {
    let epoch_secs = iso8601_to_epoch(s)?;
    let base_ms = epoch_secs.saturating_mul(1000);

    // Look for fractional seconds: '.' at position 19 (after "...SS.").
    if s.len() > 20 && s.as_bytes().get(19) == Some(&b'.') {
        let frac_start = 20;
        let frac_end = s[frac_start..]
            .find(|c: char| !c.is_ascii_digit())
            .map(|i| frac_start + i)
            .unwrap_or(s.len());
        let frac_str = &s[frac_start..frac_end];
        if !frac_str.is_empty() {
            let ms_digits = match frac_str.len() {
                1 => frac_str.parse::<u64>().ok()?.saturating_mul(100),
                2 => frac_str.parse::<u64>().ok()?.saturating_mul(10),
                _ => frac_str[..3].parse::<u64>().ok()?,
            };
            return Some(base_ms + ms_digits);
        }
    }
    Some(base_ms)
}

/// Days from 1970-01-01 to the given date (may be negative for dates before epoch).
/// Algorithm: http://howardhinnant.github.io/date_algorithms.html
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    // Shift March 1 to start of year so Feb (with leap day) is last month.
    let y = if month <= 2 { year - 1 } else { year };
    let m = month;
    let d = day;
    let era = y.div_euclid(400);
    let yoe = y - era * 400; // [0, 399]
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize env-var tests to avoid races with parallel test runner.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn from_env_returns_none_when_keys_absent() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::remove_var("LANGFUSE_PUBLIC_KEY");
            std::env::remove_var("LANGFUSE_SECRET_KEY");
        }
        assert!(LangfuseClient::from_env().is_none());
    }

    #[test]
    fn from_env_returns_some_when_keys_present() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("LANGFUSE_PUBLIC_KEY", "pk-test");
            std::env::set_var("LANGFUSE_SECRET_KEY", "sk-test");
        }
        let client = LangfuseClient::from_env();
        unsafe {
            std::env::remove_var("LANGFUSE_PUBLIC_KEY");
            std::env::remove_var("LANGFUSE_SECRET_KEY");
        }
        assert!(client.is_some());
    }

    #[test]
    fn from_env_uses_custom_host() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("LANGFUSE_PUBLIC_KEY", "pk-test");
            std::env::set_var("LANGFUSE_SECRET_KEY", "sk-test");
            std::env::set_var("LANGFUSE_HOST", "https://my-langfuse.example.com");
        }
        let client = LangfuseClient::from_env();
        unsafe {
            std::env::remove_var("LANGFUSE_PUBLIC_KEY");
            std::env::remove_var("LANGFUSE_SECRET_KEY");
            std::env::remove_var("LANGFUSE_HOST");
        }
        assert_eq!(client.unwrap().host, "https://my-langfuse.example.com");
    }

    #[test]
    fn base64_encode_empty() {
        assert_eq!(base64_encode(b""), "");
    }

    #[test]
    fn base64_encode_produces_expected_output() {
        let encoded = base64_encode(b"pk-test:sk-test");
        assert_eq!(encoded, "cGstdGVzdDpzay10ZXN0");
    }

    #[test]
    fn base64_encode_one_byte_padding() {
        assert_eq!(base64_encode(b"A"), "QQ==");
    }

    #[test]
    fn base64_encode_two_byte_padding() {
        assert_eq!(base64_encode(b"AB"), "QUI=");
    }

    #[test]
    fn base64_encode_three_bytes_no_padding() {
        assert_eq!(base64_encode(b"ABC"), "QUJD");
    }

    #[test]
    fn iso8601_to_epoch_unix_epoch() {
        let epoch = iso8601_to_epoch("1970-01-01T00:00:00Z").unwrap();
        assert_eq!(epoch, 0);
    }

    #[test]
    fn iso8601_to_epoch_parses_2026() {
        let epoch = iso8601_to_epoch("2026-03-27T10:00:00Z").unwrap();
        assert!(epoch > 1_735_689_600); // > 2025-01-01
        assert!(epoch < 1_900_000_000); // < 2030
    }

    #[test]
    fn iso8601_to_epoch_february() {
        // 1970-02-28T00:00:00Z = 58 days * 86400 = 5_011_200
        assert_eq!(iso8601_to_epoch("1970-02-28T00:00:00Z"), Some(5_011_200));
    }

    #[test]
    fn iso8601_to_epoch_returns_none_on_short_string() {
        assert!(iso8601_to_epoch("not-a-date").is_none());
        assert!(iso8601_to_epoch("").is_none());
    }

    #[test]
    fn iso8601_to_epoch_ms_retains_milliseconds() {
        let ms = iso8601_to_epoch_ms("2026-03-27T10:15:30.750Z").unwrap();
        let base_ms = iso8601_to_epoch("2026-03-27T10:15:30Z").unwrap() * 1000;
        assert_eq!(ms, base_ms + 750);
    }

    #[test]
    fn iso8601_to_epoch_ms_no_fractional() {
        let ms = iso8601_to_epoch_ms("2026-03-27T10:15:30Z").unwrap();
        let secs_ms = iso8601_to_epoch("2026-03-27T10:15:30Z").unwrap() * 1000;
        assert_eq!(ms, secs_ms);
    }

    #[test]
    fn iso8601_to_epoch_ms_microsecond_input() {
        // 6-digit fractional: should truncate to ms
        let ms = iso8601_to_epoch_ms("2026-03-27T10:15:30.123456Z").unwrap();
        let base_ms = iso8601_to_epoch("2026-03-27T10:15:30Z").unwrap() * 1000;
        assert_eq!(ms, base_ms + 123);
    }

    #[test]
    fn build_payload_structure() {
        let entry = RequestLogEntry {
            request_id: "req-123".to_string(),
            timestamp: "2026-03-27T10:00:01Z".to_string(),
            backend: "openai".to_string(),
            model_requested: Some("claude-3-haiku-20240307".to_string()),
            model_mapped: Some("gpt-4o-mini".to_string()),
            status_code: 200,
            latency_ms: 500,
            input_tokens: Some(100),
            output_tokens: Some(50),
            is_streaming: false,
            error_message: None,
            key_id: None,
            cost_usd: Some(0.001),
        };
        let payload = build_generation_payload(&entry);
        let batch = payload["batch"].as_array().unwrap();
        assert_eq!(batch.len(), 1);
        let event = &batch[0];
        assert_eq!(event["type"], "generation-create");
        let body = &event["body"];
        assert_eq!(body["model"], "gpt-4o-mini");
        assert_eq!(body["usage"]["input"], 100);
        assert_eq!(body["usage"]["output"], 50);
        assert_eq!(body["usage"]["unit"], "TOKENS");
        assert!(body["startTime"].as_str().is_some());
        assert!(body["endTime"].as_str().is_some());
        assert_eq!(body["level"], "DEFAULT");
        assert_eq!(body["metadata"]["backend"], "openai");
        assert_eq!(body["metadata"]["latency_ms"], 500);
    }

    #[test]
    fn build_payload_error_entry() {
        let entry = RequestLogEntry {
            request_id: "req-err".to_string(),
            timestamp: "2026-03-27T10:00:01Z".to_string(),
            backend: "openai".to_string(),
            model_requested: Some("gpt-4o".to_string()),
            model_mapped: None,
            status_code: 500,
            latency_ms: 100,
            input_tokens: None,
            output_tokens: None,
            is_streaming: false,
            error_message: Some("internal error".to_string()),
            key_id: None,
            cost_usd: None,
        };
        let payload = build_generation_payload(&entry);
        let body = &payload["batch"][0]["body"];
        assert_eq!(body["level"], "ERROR");
        assert_eq!(body["metadata"]["error"], "internal error");
    }

    #[test]
    fn build_payload_starttime_has_ms_precision_for_subsecond_latency() {
        let entry = RequestLogEntry {
            request_id: "req-ms".to_string(),
            timestamp: "2026-03-27T10:00:01Z".to_string(),
            backend: "openai".to_string(),
            model_requested: Some("gpt-4o".to_string()),
            model_mapped: None,
            status_code: 200,
            latency_ms: 500,
            input_tokens: None,
            output_tokens: None,
            is_streaming: false,
            error_message: None,
            key_id: None,
            cost_usd: None,
        };
        let payload = build_generation_payload(&entry);
        let body = &payload["batch"][0]["body"];
        let start = body["startTime"].as_str().unwrap();
        let end = body["endTime"].as_str().unwrap();
        assert_ne!(start, end, "sub-second latency should produce different start/end times");
        assert!(start.contains('.'), "startTime should have ms precision: {start}");
    }
}
