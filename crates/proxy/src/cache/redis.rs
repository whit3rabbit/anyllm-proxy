//! Redis L2 cache backend.
//!
//! Provides a `RedisCache` that implements `CacheBackend` for use as an
//! L2 cache behind the in-memory moka cache. Feature-gated behind `redis`.
//!
//! Graceful fallback: if Redis is unreachable, operations return None/no-op
//! and log a warning. The in-memory cache still serves as L1.

#[cfg(feature = "redis")]
use redis::aio::ConnectionManager;

#[cfg(feature = "redis")]
use super::CacheEntry;

/// Redis-backed response cache using SETEX for per-entry TTL.
#[cfg(feature = "redis")]
pub struct RedisCache {
    conn: ConnectionManager,
    /// Key prefix to namespace cache entries.
    prefix: String,
}

#[cfg(feature = "redis")]
impl RedisCache {
    /// Create a new Redis cache from an existing connection manager.
    pub fn new(conn: ConnectionManager) -> Self {
        Self {
            conn,
            prefix: "anyllm:cache:".to_string(),
        }
    }

    /// Connect to Redis and create a cache.
    pub async fn connect(redis_url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(redis_url)?;
        let conn = ConnectionManager::new(client).await?;
        Ok(Self::new(conn))
    }

    fn redis_key(&self, key: &str) -> String {
        format!("{}{}", self.prefix, key)
    }

    /// Get a cached entry from Redis.
    pub async fn get(&self, key: &str) -> Option<CacheEntry> {
        let redis_key = self.redis_key(key);
        let mut conn = self.conn.clone();
        let result: Result<Option<String>, redis::RedisError> = redis::cmd("GET")
            .arg(&redis_key)
            .query_async(&mut conn)
            .await;
        match result {
            Ok(Some(json)) => serde_json::from_str::<RedisCacheValue>(&json)
                .ok()
                .map(|v| CacheEntry {
                    response_body: bytes::Bytes::from(v.response_body),
                    model: v.model,
                    created_at: std::time::Instant::now(),
                    ttl_secs: None, // Redis manages its own TTL via SETEX
                }),
            Ok(None) => None,
            Err(e) => {
                tracing::warn!(error = %e, "Redis cache GET failed");
                None
            }
        }
    }

    /// Store a cache entry in Redis with the given TTL.
    pub async fn put(&self, key: &str, entry: &CacheEntry, ttl_secs: u64) {
        let redis_key = self.redis_key(key);
        let value = RedisCacheValue {
            response_body: String::from_utf8_lossy(&entry.response_body).to_string(),
            model: entry.model.clone(),
        };
        let json = match serde_json::to_string(&value) {
            Ok(j) => j,
            Err(_) => return,
        };
        let mut conn = self.conn.clone();
        let result: Result<(), redis::RedisError> = redis::cmd("SETEX")
            .arg(&redis_key)
            .arg(ttl_secs)
            .arg(&json)
            .query_async(&mut conn)
            .await;
        if let Err(e) = result {
            tracing::warn!(error = %e, "Redis cache SETEX failed");
        }
    }
}

/// Serializable value stored in Redis.
#[cfg(feature = "redis")]
#[derive(serde::Serialize, serde::Deserialize)]
struct RedisCacheValue {
    response_body: String,
    model: String,
}
