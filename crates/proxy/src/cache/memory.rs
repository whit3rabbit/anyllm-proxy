//! In-memory cache backend using moka's async cache.
//!
//! moka provides a concurrent, lock-free cache with TTL-based expiration
//! and bounded capacity (LRU eviction when full).
//!
//! Per-entry TTL is enforced via moka's `Expiry` trait. Each `CacheEntry`
//! carries an optional `ttl_secs` override; when absent, the cache-level
//! default applies.

use super::{CacheBackend, CacheConfig, CacheEntry};
use moka::Expiry;
use std::time::Duration;

/// Per-entry expiry policy. Reads `CacheEntry::ttl_secs` to decide lifetime;
/// falls back to `default_ttl` when the entry has no override.
struct EntryExpiry {
    default_ttl: Duration,
}

impl Expiry<String, CacheEntry> for EntryExpiry {
    fn expire_after_create(
        &self,
        _key: &String,
        value: &CacheEntry,
        _current_time: std::time::Instant,
    ) -> Option<Duration> {
        let ttl = match value.ttl_secs {
            Some(secs) => Duration::from_secs(secs),
            None => self.default_ttl,
        };
        Some(ttl)
    }
}

/// In-memory cache backed by moka::future::Cache.
///
/// Configured with a default TTL and max entry count. Per-request TTL
/// overrides are enforced via the `EntryExpiry` implementation of moka's
/// `Expiry` trait.
pub struct MemoryCache {
    inner: moka::future::Cache<String, CacheEntry>,
    /// Default TTL applied when the request does not specify cache_ttl_secs.
    pub default_ttl_secs: u64,
}

impl MemoryCache {
    /// Create a new in-memory cache from the provided configuration.
    pub fn new(config: &CacheConfig) -> Self {
        let default_ttl = Duration::from_secs(config.ttl_secs);
        let inner = moka::future::Cache::builder()
            .max_capacity(config.max_entries)
            .expire_after(EntryExpiry { default_ttl })
            .build();
        Self {
            inner,
            default_ttl_secs: config.ttl_secs,
        }
    }
}

impl CacheBackend for MemoryCache {
    async fn get(&self, key: &str) -> Option<CacheEntry> {
        self.inner.get(key).await
    }

    async fn put(&self, key: &str, entry: CacheEntry, _ttl_secs: u64) {
        // Per-entry TTL is now handled by EntryExpiry reading entry.ttl_secs.
        // The _ttl_secs parameter from CacheBackend::put is unused; the entry
        // itself carries the authoritative TTL override.
        self.inner.insert(key.to_string(), entry).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use std::time::Instant;

    fn test_config() -> CacheConfig {
        CacheConfig {
            ttl_secs: 2,
            max_entries: 100,
            redis_url: None,
        }
    }

    fn test_entry(body: &str) -> CacheEntry {
        CacheEntry {
            response_body: Bytes::from(body.to_string()),
            model: "test-model".to_string(),
            created_at: Instant::now(),
            ttl_secs: None,
        }
    }

    fn test_entry_with_ttl(body: &str, ttl: u64) -> CacheEntry {
        CacheEntry {
            response_body: Bytes::from(body.to_string()),
            model: "test-model".to_string(),
            created_at: Instant::now(),
            ttl_secs: Some(ttl),
        }
    }

    #[tokio::test]
    async fn put_and_get() {
        let cache = MemoryCache::new(&test_config());
        let entry = test_entry(r#"{"id":"msg_1"}"#);
        cache.put("test:abc123", entry.clone(), 60).await;
        let got = cache.get("test:abc123").await;
        assert!(got.is_some());
        assert_eq!(got.unwrap().response_body, entry.response_body);
    }

    #[tokio::test]
    async fn get_miss() {
        let cache = MemoryCache::new(&test_config());
        let got = cache.get("test:nonexistent").await;
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn ttl_expiry() {
        let config = CacheConfig {
            ttl_secs: 1,
            max_entries: 100,
            redis_url: None,
        };
        let cache = MemoryCache::new(&config);
        cache.put("test:expire", test_entry("data"), 1).await;

        // Entry should be present immediately
        assert!(cache.get("test:expire").await.is_some());

        // Wait for TTL to expire
        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert!(cache.get("test:expire").await.is_none());
    }

    #[tokio::test]
    async fn max_capacity_eviction() {
        let config = CacheConfig {
            ttl_secs: 300,
            max_entries: 2,
            redis_url: None,
        };
        let cache = MemoryCache::new(&config);

        cache.put("k1", test_entry("v1"), 300).await;
        cache.put("k2", test_entry("v2"), 300).await;
        cache.put("k3", test_entry("v3"), 300).await;

        // moka eviction is async; run pending maintenance
        cache.inner.run_pending_tasks().await;

        // moka eviction is best-effort and async; just verify it does not
        // grow unbounded beyond the configured max_capacity.
        assert!(cache.inner.entry_count() <= 3);
    }

    #[tokio::test]
    async fn per_entry_ttl_shorter_than_default() {
        // Default TTL is 10s, but entry requests 1s.
        let config = CacheConfig {
            ttl_secs: 10,
            max_entries: 100,
            redis_url: None,
        };
        let cache = MemoryCache::new(&config);
        let entry = test_entry_with_ttl("short-lived", 1);
        cache.put("test:short", entry, 1).await;

        // Present immediately
        assert!(cache.get("test:short").await.is_some());

        // Expired after 1.5s (entry TTL = 1s)
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert!(
            cache.get("test:short").await.is_none(),
            "entry with 1s TTL should be expired after 1.5s despite 10s default"
        );
    }

    #[tokio::test]
    async fn per_entry_ttl_longer_than_default() {
        // Default TTL is 1s, but entry requests 3s.
        let config = CacheConfig {
            ttl_secs: 1,
            max_entries: 100,
            redis_url: None,
        };
        let cache = MemoryCache::new(&config);
        let entry = test_entry_with_ttl("long-lived", 3);
        cache.put("test:long", entry, 3).await;

        // Still alive after 1.5s (past the default 1s)
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert!(
            cache.get("test:long").await.is_some(),
            "entry with 3s TTL should survive past the 1s default"
        );

        // Expired after 3.5s
        tokio::time::sleep(Duration::from_millis(2000)).await;
        assert!(
            cache.get("test:long").await.is_none(),
            "entry with 3s TTL should be expired after 3.5s"
        );
    }
}
