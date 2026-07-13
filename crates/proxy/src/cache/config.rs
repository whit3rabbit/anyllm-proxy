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
