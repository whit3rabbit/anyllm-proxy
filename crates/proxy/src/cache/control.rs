use super::CacheEntry;
use super::MAX_TTL_SECS;
use std::time::Duration;

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
