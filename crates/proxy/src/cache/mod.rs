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
use std::time::{Duration, Instant};

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
    if let Some(namespace) = scope.namespace {
        fields.push(CacheField::Str("_scope_cache_namespace", namespace));
    }
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
    // Exclude fields that do not affect the backend response:
    // - stream / stream_options: transport only (a cached non-stream response is
    //   replayed as a stream and vice versa).
    // - cache: request cache controls, handled outside response-content hashing.
    // - _scope_*: added separately as scope fields.
    // - user / metadata: tracking fields documented as "Ignored" (anyllm_translate
    //   ChatCompletionRequest user, anthropic Metadata). Hashing them fragments the
    //   cache per end-user with no correctness benefit (tenant isolation is already
    //   provided by _scope_auth).
    //
    // parallel_tool_calls is NOT excluded: backends that honor it (e.g. OpenAI)
    // produce different output for true vs false, so it must be part of the cache
    // identity. (Gemini/Vertex have it stripped before dispatch by the tool policy.)
    !matches!(
        key,
        "stream"
            | "stream_options"
            | "cache"
            | "_scope_auth"
            | "_scope_backend"
            | "_scope_cache_namespace"
            | "user"
            | "metadata"
    )
}

pub struct CacheScope<'a> {
    pub backend_name: &'a str,
    pub auth_identity: &'a str,
    pub namespace: Option<&'a str>,
}

/// Per-request cache controls after combining local and LiteLLM-style fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheControl {
    /// Whether to read an existing cached response before calling upstream.
    pub lookup: bool,
    /// Whether to store a successful upstream response after the call.
    pub store: bool,
    /// Per-entry store TTL override in seconds.
    pub ttl_secs: Option<u64>,
    /// Maximum acceptable age for an existing cached entry.
    pub max_age_secs: Option<u64>,
    /// Optional caller namespace for exact-match cache isolation.
    pub namespace: Option<String>,
    /// Parsed for LiteLLM compatibility. Currently no behavior change.
    pub use_cache: bool,
}

impl Default for CacheControl {
    fn default() -> Self {
        Self {
            lookup: true,
            store: true,
            ttl_secs: None,
            max_age_secs: None,
            namespace: None,
            use_cache: false,
        }
    }
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

/// Parse local `cache_ttl_secs` and LiteLLM-compatible top-level `cache`.
pub fn parse_cache_control(body: &serde_json::Value) -> Result<CacheControl, String> {
    let mut control = CacheControl::default();
    match parse_cache_ttl(body)? {
        Some(0) => {
            control.lookup = false;
            control.store = false;
            control.ttl_secs = Some(0);
        }
        Some(ttl) => {
            control.ttl_secs = Some(ttl);
        }
        None => {}
    }

    let Some(cache_value) = body.get("cache") else {
        return Ok(control);
    };
    if cache_value.is_null() {
        return Ok(control);
    }
    let Some(cache_obj) = cache_value.as_object() else {
        return Err(format!("cache must be an object, got {}", cache_value));
    };

    if parse_cache_bool_field(cache_obj, "no-cache")?.unwrap_or(false) {
        control.lookup = false;
    }
    if parse_cache_bool_field(cache_obj, "no-store")?.unwrap_or(false) {
        control.store = false;
    }
    if let Some(use_cache) = parse_cache_bool_field(cache_obj, "use-cache")? {
        control.use_cache = use_cache;
    }
    if let Some(ttl) = parse_cache_secs_field(cache_obj, "ttl")? {
        control.ttl_secs = Some(ttl);
    }
    if let Some(max_age) = parse_cache_secs_field(cache_obj, "s-maxage")? {
        control.max_age_secs = Some(max_age);
    }
    if let Some(max_age) = parse_cache_secs_field(cache_obj, "s-max-age")? {
        control.max_age_secs = Some(max_age);
    }
    if let Some(namespace_value) = cache_obj.get("namespace") {
        if !namespace_value.is_null() {
            let Some(namespace) = namespace_value.as_str() else {
                return Err(format!(
                    "cache.namespace must be a string, got {}",
                    namespace_value
                ));
            };
            if !namespace.is_empty() {
                control.namespace = Some(namespace.to_string());
            }
        }
    }

    Ok(control)
}

fn parse_cache_bool_field(
    cache_obj: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<Option<bool>, String> {
    let Some(value) = cache_obj.get(field) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_bool()
        .map(Some)
        .ok_or_else(|| format!("cache.{field} must be a boolean, got {value}"))
}

fn parse_cache_secs_field(
    cache_obj: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<Option<u64>, String> {
    let Some(value) = cache_obj.get(field) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    if let Some(n) = value.as_u64() {
        if n > MAX_TTL_SECS {
            return Err(format!("cache.{field} must be <= {MAX_TTL_SECS}, got {n}"));
        }
        return Ok(Some(n));
    }
    if let Some(n) = value.as_i64() {
        return Err(format!("cache.{field} must be non-negative, got {n}"));
    }
    if let Some(n) = value.as_f64() {
        if n < 0.0 {
            return Err(format!("cache.{field} must be non-negative, got {n}"));
        }
        let truncated = n as u64;
        if truncated > MAX_TTL_SECS {
            return Err(format!(
                "cache.{field} must be <= {MAX_TTL_SECS}, got {truncated}"
            ));
        }
        return Ok(Some(truncated));
    }
    Err(format!("cache.{field} must be a number, got {}", value))
}

pub fn cache_entry_is_fresh(entry: &CacheEntry, max_age_secs: Option<u64>) -> bool {
    match max_age_secs {
        Some(max_age) => entry.created_at.elapsed() <= Duration::from_secs(max_age),
        None => true,
    }
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
                namespace: None,
            },
        );
        let key2 = cache_key_for_request(
            &body,
            CacheNamespace::Anthropic,
            &CacheScope {
                backend_name: "openai",
                auth_identity: "k1",
                namespace: None,
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
                namespace: None,
            },
        );
        let key2 = cache_key_for_request(
            &body2,
            CacheNamespace::Anthropic,
            &CacheScope {
                backend_name: "openai",
                auth_identity: "k1",
                namespace: None,
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
                namespace: None,
            },
        );
        let key2 = cache_key_for_request(
            &body2,
            CacheNamespace::OpenAI,
            &CacheScope {
                backend_name: "openai",
                auth_identity: "k1",
                namespace: None,
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
                namespace: None,
            },
        );
        let key2 = cache_key_for_request(
            &body2,
            CacheNamespace::OpenAI,
            &CacheScope {
                backend_name: "openai",
                auth_identity: "k1",
                namespace: None,
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
                namespace: None,
            },
        );
        let oai = cache_key_for_request(
            &body,
            CacheNamespace::OpenAI,
            &CacheScope {
                backend_name: "openai",
                auth_identity: "k1",
                namespace: None,
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
                namespace: None,
            },
        );
        let key2 = cache_key_for_request(
            &body2,
            CacheNamespace::OpenAI,
            &CacheScope {
                backend_name: "openai",
                auth_identity: "k1",
                namespace: None,
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
                namespace: None,
            },
        );
        let key2 = cache_key_for_request(
            &body2,
            CacheNamespace::OpenAI,
            &CacheScope {
                backend_name: "openai",
                auth_identity: "k1",
                namespace: None,
            },
        );
        assert_ne!(
            key1, key2,
            "different cache_ttl_secs must produce different cache keys"
        );
    }

    #[test]
    fn cache_key_ignores_litellm_cache_controls() {
        let body1 = serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}],
            "cache": {"ttl": 60, "no-cache": true}
        });
        let body2 = serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}],
            "cache": {"ttl": 3600, "no-store": true}
        });

        assert_eq!(openai_key(&body1), openai_key(&body2));
    }

    #[test]
    fn cache_key_namespace_control_separates_keys() {
        let body = serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let key1 = cache_key_for_request(
            &body,
            CacheNamespace::OpenAI,
            &CacheScope {
                backend_name: "openai",
                auth_identity: "k1",
                namespace: Some("tenant-a"),
            },
        );
        let key2 = cache_key_for_request(
            &body,
            CacheNamespace::OpenAI,
            &CacheScope {
                backend_name: "openai",
                auth_identity: "k1",
                namespace: Some("tenant-b"),
            },
        );

        assert_ne!(key1, key2);
    }

    #[test]
    fn parse_cache_control_litellm_fields() {
        let body = serde_json::json!({
            "cache": {
                "ttl": 120,
                "no-cache": true,
                "no-store": false,
                "s-maxage": 30,
                "namespace": "tenant-a",
                "use-cache": true
            }
        });

        let control = parse_cache_control(&body).unwrap();

        assert!(!control.lookup);
        assert!(control.store);
        assert_eq!(control.ttl_secs, Some(120));
        assert_eq!(control.max_age_secs, Some(30));
        assert_eq!(control.namespace.as_deref(), Some("tenant-a"));
        assert!(control.use_cache);
    }

    #[test]
    fn parse_cache_control_preserves_cache_ttl_secs_bypass() {
        let body = serde_json::json!({
            "cache_ttl_secs": 0,
            "cache": {"ttl": 120}
        });

        let control = parse_cache_control(&body).unwrap();

        assert!(!control.lookup);
        assert!(!control.store);
        assert_eq!(control.ttl_secs, Some(120));
    }

    #[test]
    fn parse_cache_control_rejects_invalid_cache_object() {
        let body = serde_json::json!({"cache": true});

        assert!(parse_cache_control(&body).is_err());
    }

    #[test]
    fn cache_entry_s_maxage_rejects_stale_entries() {
        let entry = CacheEntry {
            response_body: Bytes::from_static(b"{}"),
            model: "gpt-4o".to_string(),
            created_at: Instant::now() - std::time::Duration::from_secs(10),
            ttl_secs: None,
        };

        assert!(!cache_entry_is_fresh(&entry, Some(5)));
        assert!(cache_entry_is_fresh(&entry, Some(30)));
        assert!(cache_entry_is_fresh(&entry, None));
    }

    fn test_scope() -> CacheScope<'static> {
        CacheScope {
            backend_name: "openai",
            auth_identity: "k1",
            namespace: None,
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

    #[test]
    fn cache_key_ignores_tracking_fields() {
        let base = serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let with_user = serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}],
            "user": "end-user-123"
        });
        let with_metadata = serde_json::json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 128,
            "messages": [{"role": "user", "content": "hi"}],
            "metadata": {"user_id": "session-abc"}
        });
        let metadata_base = serde_json::json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 128,
            "messages": [{"role": "user", "content": "hi"}]
        });

        assert_eq!(openai_key(&base), openai_key(&with_user));
        assert_eq!(anthropic_key(&metadata_base), anthropic_key(&with_metadata));
    }

    #[test]
    fn cache_key_includes_parallel_tool_calls() {
        let base = serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "lookup",
                    "description": "lookup",
                    "parameters": {"type": "object", "properties": {}}
                }
            }]
        });
        let with_parallel_tool_calls = serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "lookup",
                    "description": "lookup",
                    "parameters": {"type": "object", "properties": {}}
                }
            }],
            "parallel_tool_calls": false
        });

        assert_ne!(openai_key(&base), openai_key(&with_parallel_tool_calls));
    }
}
