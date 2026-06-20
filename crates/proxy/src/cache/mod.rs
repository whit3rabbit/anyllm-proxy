//! Response caching for non-streaming requests.
//!
//! Cache keys are SHA-256 hashes of the canonical (sorted-key) JSON of
//! request fields that affect the response: model, messages, temperature,
//! top_p, max_tokens, stop, tools, tool_choice.
//!
//! Two namespaces avoid cross-endpoint collisions:
//! - `anth:` for /v1/messages
//! - `oai:` for /v1/chat/completions

pub mod memory;
/// Redis L2 cache backend (requires `redis` feature).
pub mod redis;
/// Semantic cache backed by Qdrant vector store (requires `qdrant` feature).
#[cfg(feature = "qdrant")]
pub mod semantic;

use bytes::Bytes;
use sha2::{Digest, Sha256};
use std::io::{self, Write};
use std::time::Instant;

/// Maximum allowed value for per-request `cache_ttl_secs`.
pub const MAX_TTL_SECS: u64 = 86_400;

/// Cached response entry stored in any cache backend.
#[derive(Clone, Debug)]
pub struct CacheEntry {
    /// Serialized response body (JSON bytes).
    pub response_body: Bytes,
    /// Model name from the response, for diagnostics/logging.
    pub model: String,
    /// When this entry was created (wall-clock, not persisted to Redis).
    pub created_at: Instant,
    /// Per-entry TTL override in seconds. When set, moka's Expiry trait
    /// uses this instead of the cache-level default.
    pub ttl_secs: Option<u64>,
}

/// Namespace prefix for cache keys, preventing cross-endpoint collisions.
#[derive(Debug, Clone, Copy)]
pub enum CacheNamespace {
    /// Anthropic /v1/messages endpoint.
    Anthropic,
    /// OpenAI /v1/chat/completions endpoint.
    OpenAI,
}

impl CacheNamespace {
    fn prefix(self) -> &'static str {
        match self {
            Self::Anthropic => "anth",
            Self::OpenAI => "oai",
        }
    }
}

/// Pluggable cache backend trait. Implementations must be Send + Sync
/// for use behind Arc in axum handlers.
pub trait CacheBackend: Send + Sync {
    /// Look up a cached response by key. Returns None on miss.
    fn get(&self, key: &str) -> impl std::future::Future<Output = Option<CacheEntry>> + Send;

    /// Store a response in the cache with the given TTL.
    fn put(
        &self,
        key: &str,
        entry: CacheEntry,
        ttl_secs: u64,
    ) -> impl std::future::Future<Output = ()> + Send;
}

/// Compute a deterministic cache key for a request body.
///
/// Extracts the fields that affect response content, sorts them via BTreeMap,
/// serializes to canonical JSON, SHA-256 hashes the result, and prepends the
/// namespace prefix.
pub fn cache_key_for_request(
    body: &serde_json::Value,
    ns: CacheNamespace,
    scope: &CacheScope<'_>,
) -> String {
    let mut hasher = Sha256::new();
    write_canonical_cache_body(&mut hasher, body, scope);
    let hash = hasher.finalize();
    let hex = hex::encode(hash);
    format!("{}:{}", ns.prefix(), hex)
}

enum CacheField<'a> {
    Json(&'a str, &'a serde_json::Value),
    Str(&'static str, &'a str),
}

impl<'a> CacheField<'a> {
    fn key(&self) -> &str {
        match self {
            Self::Json(key, _) | Self::Str(key, _) => key,
        }
    }
}

struct HashWriter<'a> {
    hasher: &'a mut Sha256,
}

impl Write for HashWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.hasher.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn write_canonical_cache_body(
    hasher: &mut Sha256,
    body: &serde_json::Value,
    scope: &CacheScope<'_>,
) {
    let mut fields = Vec::new();
    if let Some(obj) = body.as_object() {
        fields.extend(
            obj.iter()
                .filter(|(key, value)| should_include_cache_field(key, value))
                .map(|(key, value)| CacheField::Json(key.as_str(), value)),
        );
    }
    fields.push(CacheField::Str("_scope_auth", scope.auth_identity));
    fields.push(CacheField::Str("_scope_backend", scope.backend_name));
    fields.sort_unstable_by(|a, b| a.key().cmp(b.key()));

    let mut writer = HashWriter { hasher };
    writer
        .write_all(b"{")
        .expect("hash writer should not fail writing object start");
    for (idx, field) in fields.iter().enumerate() {
        if idx > 0 {
            writer
                .write_all(b",")
                .expect("hash writer should not fail writing separator");
        }
        serde_json::to_writer(&mut writer, field.key())
            .expect("hash writer should not fail writing key");
        writer
            .write_all(b":")
            .expect("hash writer should not fail writing colon");
        match field {
            CacheField::Json(_, value) => serde_json::to_writer(&mut writer, value)
                .expect("hash writer should not fail writing JSON value"),
            CacheField::Str(_, value) => serde_json::to_writer(&mut writer, value)
                .expect("hash writer should not fail writing string value"),
        }
    }
    writer
        .write_all(b"}")
        .expect("hash writer should not fail writing object end");
}

fn should_include_cache_field(key: &str, value: &serde_json::Value) -> bool {
    if value.is_null() {
        return false;
    }
    !matches!(
        key,
        "stream" | "stream_options" | "_scope_auth" | "_scope_backend"
    )
}

pub struct CacheScope<'a> {
    pub backend_name: &'a str,
    pub auth_identity: &'a str,
}

/// Parse the optional `cache_ttl_secs` field from a request body.
///
/// Returns:
/// - `Ok(None)` if the field is absent or null (use default TTL).
/// - `Ok(Some(0))` if explicitly 0 (bypass cache).
/// - `Ok(Some(n))` for valid positive values up to MAX_TTL_SECS.
/// - `Err(message)` for negative values, values > MAX_TTL_SECS, or non-numeric.
pub fn parse_cache_ttl(body: &serde_json::Value) -> Result<Option<u64>, String> {
    let Some(val) = body.get("cache_ttl_secs") else {
        return Ok(None);
    };
    if val.is_null() {
        return Ok(None);
    }
    if let Some(n) = val.as_u64() {
        if n > MAX_TTL_SECS {
            return Err(format!("cache_ttl_secs must be <= {MAX_TTL_SECS}, got {n}"));
        }
        return Ok(Some(n));
    }
    if let Some(n) = val.as_i64() {
        // Negative values are invalid
        return Err(format!("cache_ttl_secs must be non-negative, got {n}"));
    }
    if let Some(n) = val.as_f64() {
        if n < 0.0 {
            return Err(format!("cache_ttl_secs must be non-negative, got {n}"));
        }
        let truncated = n as u64;
        if truncated > MAX_TTL_SECS {
            return Err(format!(
                "cache_ttl_secs must be <= {MAX_TTL_SECS}, got {truncated}"
            ));
        }
        return Ok(Some(truncated));
    }
    Err(format!("cache_ttl_secs must be a number, got {}", val))
}

/// Configuration for the cache subsystem.
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Default TTL in seconds for cached responses.
    pub ttl_secs: u64,
    /// Maximum number of entries in the in-memory cache.
    pub max_entries: u64,
    /// Optional Redis URL. Used by the Redis L2 cache backend (requires `redis` feature)
    /// and distributed rate limiting. When set, responses are cached in Redis in addition
    /// to the in-memory L1 cache, and rate limit state is shared across proxy instances.
    pub redis_url: Option<String>,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            ttl_secs: 300,
            max_entries: 10_000,
            redis_url: None,
        }
    }
}

impl CacheConfig {
    /// Load cache configuration from environment variables.
    pub fn from_env() -> Self {
        let ttl_secs = std::env::var("CACHE_TTL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(300);
        let max_entries = std::env::var("CACHE_MAX_ENTRIES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10_000);
        let redis_url = std::env::var("REDIS_URL").ok();
        Self {
            ttl_secs,
            max_entries,
            redis_url,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_deterministic_same_fields() {
        let body = serde_json::json!({
            "model": "claude-sonnet-4-6",
            "messages": [{"role": "user", "content": "hello"}],
            "temperature": 0.7,
            "max_tokens": 100
        });
        let key1 = cache_key_for_request(
            &body,
            CacheNamespace::Anthropic,
            &CacheScope {
                backend_name: "openai",
                auth_identity: "k1",
            },
        );
        let key2 = cache_key_for_request(
            &body,
            CacheNamespace::Anthropic,
            &CacheScope {
                backend_name: "openai",
                auth_identity: "k1",
            },
        );
        assert_eq!(key1, key2);
        assert!(key1.starts_with("anth:"));
    }

    #[test]
    fn cache_key_different_for_different_temperature() {
        let body1 = serde_json::json!({
            "model": "claude-sonnet-4-6",
            "messages": [{"role": "user", "content": "hello"}],
            "temperature": 0.7
        });
        let body2 = serde_json::json!({
            "model": "claude-sonnet-4-6",
            "messages": [{"role": "user", "content": "hello"}],
            "temperature": 0.9
        });
        let key1 = cache_key_for_request(
            &body1,
            CacheNamespace::Anthropic,
            &CacheScope {
                backend_name: "openai",
                auth_identity: "k1",
            },
        );
        let key2 = cache_key_for_request(
            &body2,
            CacheNamespace::Anthropic,
            &CacheScope {
                backend_name: "openai",
                auth_identity: "k1",
            },
        );
        assert_ne!(key1, key2);
    }

    #[test]
    fn cache_key_ignores_field_order() {
        // JSON object field order should not affect the key because we
        // extract into a BTreeMap.
        let body1 = serde_json::json!({
            "model": "gpt-4o",
            "temperature": 0.5,
            "messages": [{"role": "user", "content": "hi"}]
        });
        let body2 = serde_json::json!({
            "messages": [{"role": "user", "content": "hi"}],
            "model": "gpt-4o",
            "temperature": 0.5
        });
        let key1 = cache_key_for_request(
            &body1,
            CacheNamespace::OpenAI,
            &CacheScope {
                backend_name: "openai",
                auth_identity: "k1",
            },
        );
        let key2 = cache_key_for_request(
            &body2,
            CacheNamespace::OpenAI,
            &CacheScope {
                backend_name: "openai",
                auth_identity: "k1",
            },
        );
        assert_eq!(key1, key2);
    }

    #[test]
    fn cache_key_ignores_non_cache_fields() {
        let body1 = serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}],
            "stream": true
        });
        let body2 = serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let key1 = cache_key_for_request(
            &body1,
            CacheNamespace::OpenAI,
            &CacheScope {
                backend_name: "openai",
                auth_identity: "k1",
            },
        );
        let key2 = cache_key_for_request(
            &body2,
            CacheNamespace::OpenAI,
            &CacheScope {
                backend_name: "openai",
                auth_identity: "k1",
            },
        );
        assert_eq!(key1, key2);
    }

    #[test]
    fn cache_key_namespace_differs() {
        let body = serde_json::json!({
            "model": "test",
            "messages": []
        });
        let anth = cache_key_for_request(
            &body,
            CacheNamespace::Anthropic,
            &CacheScope {
                backend_name: "openai",
                auth_identity: "k1",
            },
        );
        let oai = cache_key_for_request(
            &body,
            CacheNamespace::OpenAI,
            &CacheScope {
                backend_name: "openai",
                auth_identity: "k1",
            },
        );
        assert_ne!(anth, oai);
        assert!(anth.starts_with("anth:"));
        assert!(oai.starts_with("oai:"));
    }

    #[test]
    fn cache_key_null_field_same_as_absent() {
        let body1 = serde_json::json!({
            "model": "gpt-4o",
            "messages": [],
            "temperature": null
        });
        let body2 = serde_json::json!({
            "model": "gpt-4o",
            "messages": []
        });
        let key1 = cache_key_for_request(
            &body1,
            CacheNamespace::OpenAI,
            &CacheScope {
                backend_name: "openai",
                auth_identity: "k1",
            },
        );
        let key2 = cache_key_for_request(
            &body2,
            CacheNamespace::OpenAI,
            &CacheScope {
                backend_name: "openai",
                auth_identity: "k1",
            },
        );
        assert_eq!(key1, key2);
    }

    #[test]
    fn parse_cache_ttl_absent() {
        let body = serde_json::json!({"model": "test"});
        assert_eq!(parse_cache_ttl(&body).unwrap(), None);
    }

    #[test]
    fn parse_cache_ttl_null() {
        let body = serde_json::json!({"cache_ttl_secs": null});
        assert_eq!(parse_cache_ttl(&body).unwrap(), None);
    }

    #[test]
    fn parse_cache_ttl_zero() {
        let body = serde_json::json!({"cache_ttl_secs": 0});
        assert_eq!(parse_cache_ttl(&body).unwrap(), Some(0));
    }

    #[test]
    fn parse_cache_ttl_valid() {
        let body = serde_json::json!({"cache_ttl_secs": 600});
        assert_eq!(parse_cache_ttl(&body).unwrap(), Some(600));
    }

    #[test]
    fn parse_cache_ttl_max() {
        let body = serde_json::json!({"cache_ttl_secs": 86400});
        assert_eq!(parse_cache_ttl(&body).unwrap(), Some(86400));
    }

    #[test]
    fn parse_cache_ttl_over_max() {
        let body = serde_json::json!({"cache_ttl_secs": 86401});
        assert!(parse_cache_ttl(&body).is_err());
    }

    #[test]
    fn parse_cache_ttl_negative() {
        let body = serde_json::json!({"cache_ttl_secs": -1});
        assert!(parse_cache_ttl(&body).is_err());
    }

    #[test]
    fn parse_cache_ttl_string() {
        let body = serde_json::json!({"cache_ttl_secs": "not a number"});
        assert!(parse_cache_ttl(&body).is_err());
    }

    #[test]
    fn cache_key_differs_for_different_cache_ttl_secs() {
        let body1 = serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}],
            "cache_ttl_secs": 60
        });
        let body2 = serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}],
            "cache_ttl_secs": 3600
        });
        let key1 = cache_key_for_request(
            &body1,
            CacheNamespace::OpenAI,
            &CacheScope {
                backend_name: "openai",
                auth_identity: "k1",
            },
        );
        let key2 = cache_key_for_request(
            &body2,
            CacheNamespace::OpenAI,
            &CacheScope {
                backend_name: "openai",
                auth_identity: "k1",
            },
        );
        assert_ne!(
            key1, key2,
            "different cache_ttl_secs must produce different cache keys"
        );
    }

    fn test_scope() -> CacheScope<'static> {
        CacheScope {
            backend_name: "openai",
            auth_identity: "k1",
        }
    }

    fn anthropic_key(body: &serde_json::Value) -> String {
        cache_key_for_request(body, CacheNamespace::Anthropic, &test_scope())
    }

    fn openai_key(body: &serde_json::Value) -> String {
        cache_key_for_request(body, CacheNamespace::OpenAI, &test_scope())
    }

    #[test]
    fn cache_key_includes_anthropic_response_affecting_fields() {
        let base = serde_json::json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 128,
            "messages": [{"role": "user", "content": "hi"}]
        });

        let with_top_k = serde_json::json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 128,
            "messages": [{"role": "user", "content": "hi"}],
            "top_k": 10
        });
        let with_stop_sequences = serde_json::json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 128,
            "messages": [{"role": "user", "content": "hi"}],
            "stop_sequences": ["END"]
        });
        let with_thinking = serde_json::json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 128,
            "messages": [{"role": "user", "content": "hi"}],
            "thinking": {"type": "enabled", "budget_tokens": 1024}
        });

        assert_ne!(anthropic_key(&base), anthropic_key(&with_top_k));
        assert_ne!(anthropic_key(&base), anthropic_key(&with_stop_sequences));
        assert_ne!(anthropic_key(&base), anthropic_key(&with_thinking));
    }

    #[test]
    fn cache_key_includes_unknown_extra_fields() {
        let base = serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let with_extra = serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}],
            "prediction": {"type": "content", "content": "expected"}
        });

        assert_ne!(openai_key(&base), openai_key(&with_extra));
    }
}
