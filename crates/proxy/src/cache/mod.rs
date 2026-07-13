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

mod config;
mod control;
mod key;

#[cfg(test)]
mod tests;

use bytes::Bytes;
use std::time::Instant;

/// Maximum allowed value for per-request `cache_ttl_secs`.
pub const MAX_TTL_SECS: u64 = 86_400;

pub use config::CacheConfig;
pub use control::{cache_entry_is_fresh, parse_cache_control, parse_cache_ttl, CacheControl};
pub use key::{cache_key_for_request, CacheNamespace, CacheScope};

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
