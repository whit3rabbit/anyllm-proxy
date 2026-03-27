//! Distributed rate limiting via Redis sorted sets.
//!
//! When `REDIS_URL` is set and the `redis` feature is enabled, RPM/TPM
//! checks are performed against Redis so multiple proxy instances share
//! rate limit state. On Redis failure, the proxy falls back to local
//! in-memory rate limiting (each instance limits independently).

/// Policy for handling Redis rate limiter errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitFailPolicy {
    /// Allow requests when Redis is unavailable (default).
    Open,
    /// Reject requests when Redis is unavailable.
    Closed,
}

impl RateLimitFailPolicy {
    pub fn from_env_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "closed" => Self::Closed,
            _ => Self::Open,
        }
    }

    pub fn from_env() -> Self {
        std::env::var("RATE_LIMIT_FAIL_POLICY")
            .map(|v| Self::from_env_str(&v))
            .unwrap_or(Self::Open)
    }
}

#[cfg(feature = "redis")]
use redis::aio::ConnectionManager;
#[cfg(feature = "redis")]
use std::sync::{LazyLock, OnceLock};

#[cfg(feature = "redis")]
static REDIS_RATE_LIMITER: OnceLock<RedisRateLimiter> = OnceLock::new();

/// Initialize the global Redis rate limiter. Called once from main.
#[cfg(feature = "redis")]
pub fn set_redis_rate_limiter(limiter: RedisRateLimiter) {
    let _ = REDIS_RATE_LIMITER.set(limiter);
}

/// Get the global Redis rate limiter, if initialized.
#[cfg(feature = "redis")]
pub fn get_redis_rate_limiter() -> Option<&'static RedisRateLimiter> {
    REDIS_RATE_LIMITER.get()
}

/// Stub when redis feature is not enabled.
#[cfg(not(feature = "redis"))]
pub fn get_redis_rate_limiter() -> Option<&'static ()> {
    None
}

/// Redis-backed distributed rate limiter using sorted sets.
///
/// Keys use the format `anyllm:rl:{key_hash_hex}:rpm` and `anyllm:rl:{key_hash_hex}:tpm`.
/// Each request is a member scored by its timestamp in milliseconds.
/// A Lua script atomically trims expired entries, checks the count/sum,
/// and adds the new entry if within limits.
#[cfg(feature = "redis")]
pub struct RedisRateLimiter {
    conn: ConnectionManager,
    fail_policy: RateLimitFailPolicy,
}

#[cfg(feature = "redis")]
impl RedisRateLimiter {
    /// Connect to Redis and create a rate limiter.
    pub async fn new(redis_url: &str, fail_policy: RateLimitFailPolicy) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(redis_url)?;
        let conn = ConnectionManager::new(client).await?;
        Ok(Self { conn, fail_policy })
    }

    /// Get the underlying connection manager for reuse (e.g., by cache layer).
    pub fn connection(&self) -> &ConnectionManager {
        &self.conn
    }

    /// Check RPM limit. Returns Ok(()) if allowed, Err(retry_after_secs) if exceeded.
    /// On Redis error, behavior depends on the configured `RateLimitFailPolicy`.
    pub async fn check_rpm(&self, key_hash_hex: &str, limit: u32, now_ms: u64) -> Result<(), u64> {
        let redis_key = format!("anyllm:rl:{key_hash_hex}:rpm");
        match self.check_rpm_inner(&redis_key, limit, now_ms).await {
            Ok(result) => result,
            Err(e) => match self.fail_policy {
                RateLimitFailPolicy::Open => {
                    tracing::warn!(error = %e, "Redis RPM check failed, allowing request (fail-open)");
                    Ok(())
                }
                RateLimitFailPolicy::Closed => {
                    tracing::error!(error = %e, "Redis RPM check failed, rejecting request (fail-closed)");
                    Err(1)
                }
            },
        }
    }

    async fn check_rpm_inner(
        &self,
        redis_key: &str,
        limit: u32,
        now_ms: u64,
    ) -> Result<Result<(), u64>, redis::RedisError> {
        let mut conn = self.conn.clone();
        let cutoff = now_ms.saturating_sub(60_000);
        let member_id = format!("{now_ms}:{}", uuid::Uuid::new_v4().as_simple());

        // Hashed once at first use; avoids re-computing SHA1 per request.
        static RPM_SCRIPT: LazyLock<redis::Script> = LazyLock::new(|| {
            redis::Script::new(
                r#"
                redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', ARGV[1])
                local count = redis.call('ZCARD', KEYS[1])
                if count >= tonumber(ARGV[2]) then
                    local oldest = redis.call('ZRANGE', KEYS[1], 0, 0, 'WITHSCORES')
                    if oldest and #oldest >= 2 then
                        return oldest[2]
                    end
                    return tostring(ARGV[3])
                end
                redis.call('ZADD', KEYS[1], ARGV[3], ARGV[4])
                redis.call('EXPIRE', KEYS[1], 120)
                return 0
                "#,
            )
        });

        let result: i64 = RPM_SCRIPT
            .key(redis_key)
            .arg(cutoff)
            .arg(limit)
            .arg(now_ms)
            .arg(&member_id)
            .invoke_async(&mut conn)
            .await?;

        if result == 0 {
            Ok(Ok(()))
        } else {
            let oldest_ms = result as u64;
            let retry_after_ms = (oldest_ms + 60_000).saturating_sub(now_ms);
            Ok(Err((retry_after_ms / 1000).max(1)))
        }
    }

    /// Check TPM limit. Returns Ok(()) if allowed, Err(retry_after_secs) if exceeded.
    /// On Redis error, behavior depends on the configured `RateLimitFailPolicy`.
    pub async fn check_tpm(&self, key_hash_hex: &str, limit: u32, now_ms: u64) -> Result<(), u64> {
        let redis_key = format!("anyllm:rl:{key_hash_hex}:tpm");
        match self.check_tpm_inner(&redis_key, limit, now_ms).await {
            Ok(result) => result,
            Err(e) => match self.fail_policy {
                RateLimitFailPolicy::Open => {
                    tracing::warn!(error = %e, "Redis TPM check failed, allowing request (fail-open)");
                    Ok(())
                }
                RateLimitFailPolicy::Closed => {
                    tracing::error!(error = %e, "Redis TPM check failed, rejecting request (fail-closed)");
                    Err(1)
                }
            },
        }
    }

    async fn check_tpm_inner(
        &self,
        redis_key: &str,
        limit: u32,
        now_ms: u64,
    ) -> Result<Result<(), u64>, redis::RedisError> {
        let mut conn = self.conn.clone();
        let cutoff = now_ms.saturating_sub(60_000);

        // For TPM, members are scored by timestamp and the member value encodes the token count.
        // We sum member names (which are "{tokens}:{uuid}") to get total tokens.
        static TPM_SCRIPT: LazyLock<redis::Script> = LazyLock::new(|| {
            redis::Script::new(
                r#"
                redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', ARGV[1])
                local members = redis.call('ZRANGE', KEYS[1], 0, -1)
                local total = 0
                for _, m in ipairs(members) do
                    local tokens = tonumber(string.match(m, '^(%d+):'))
                    if tokens then total = total + tokens end
                end
                if total >= tonumber(ARGV[2]) then
                    local oldest = redis.call('ZRANGE', KEYS[1], 0, 0, 'WITHSCORES')
                    if oldest and #oldest >= 2 then
                        return oldest[2]
                    end
                    return tostring(ARGV[3])
                end
                return 0
                "#,
            )
        });

        let result: i64 = TPM_SCRIPT
            .key(redis_key)
            .arg(cutoff)
            .arg(limit)
            .arg(now_ms)
            .invoke_async(&mut conn)
            .await?;

        if result == 0 {
            Ok(Ok(()))
        } else {
            let oldest_ms = result as u64;
            let retry_after_ms = (oldest_ms + 60_000).saturating_sub(now_ms);
            Ok(Err((retry_after_ms / 1000).max(1)))
        }
    }

    /// Record TPM tokens after a response is received.
    pub async fn record_tpm(&self, key_hash_hex: &str, now_ms: u64, tokens: u32) {
        let redis_key = format!("anyllm:rl:{key_hash_hex}:tpm");
        let member = format!("{tokens}:{}", uuid::Uuid::new_v4().as_simple());
        let mut conn = self.conn.clone();
        let result: Result<(), redis::RedisError> = redis::pipe()
            .zadd(&redis_key, member, now_ms as f64)
            .expire(&redis_key, 120)
            .query_async(&mut conn)
            .await;
        if let Err(e) = result {
            tracing::warn!(error = %e, "Redis TPM record failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RateLimitFailPolicy;

    #[test]
    fn get_redis_rate_limiter_returns_none_without_init() {
        // When redis feature is not enabled, or when not initialized,
        // the function should return None.
        assert!(super::get_redis_rate_limiter().is_none());
    }

    #[test]
    fn parse_rate_limit_fail_policy() {
        assert!(matches!(
            RateLimitFailPolicy::from_env_str("open"),
            RateLimitFailPolicy::Open
        ));
        assert!(matches!(
            RateLimitFailPolicy::from_env_str("closed"),
            RateLimitFailPolicy::Closed
        ));
        assert!(matches!(
            RateLimitFailPolicy::from_env_str("OPEN"),
            RateLimitFailPolicy::Open
        ));
        assert!(matches!(
            RateLimitFailPolicy::from_env_str("CLOSED"),
            RateLimitFailPolicy::Closed
        ));
        assert!(matches!(
            RateLimitFailPolicy::from_env_str("unknown"),
            RateLimitFailPolicy::Open
        ));
    }
}
