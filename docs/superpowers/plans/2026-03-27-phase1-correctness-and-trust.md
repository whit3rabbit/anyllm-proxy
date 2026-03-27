# Phase 1: Correctness and Trust Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the six highest-priority correctness and production-safety issues identified in the LiteLLM competitive gap analysis, bringing the proxy from "directionally correct" to "trustworthy in production."

**Architecture:** Each task is independent and can be worked in any order. Tasks 1-2 are pure bug fixes in existing modules. Tasks 3-5 add new counters, config knobs, and auth modes to existing middleware. Task 6 adds a live integration test file. No new crates or major structural changes.

**Tech Stack:** Rust stable (1.83+), moka 0.12 (Expiry trait), axum 0.8, redis 0.27, jsonwebtoken 9, tokio, serde

---

## Task 1: Fix Per-Entry Cache TTL in MemoryCache

**Problem:** `MemoryCache::put()` accepts `ttl_secs` but ignores it (`let _ = ttl_secs`). All entries use the global cache-level TTL. Redis backend already respects per-entry TTL via SETEX. This means request-level `cache_ttl_secs` is parsed and validated but never enforced in-memory.

**Fix:** Implement moka's `Expiry` trait to override TTL per entry. Store the requested TTL inside `CacheEntry` so the expiry callback can read it.

**Files:**
- Modify: `crates/proxy/src/cache/mod.rs` (add `ttl_secs` field to `CacheEntry`)
- Modify: `crates/proxy/src/cache/memory.rs` (implement `Expiry`, use `policy_insert_with_expiry`)
- Modify: `crates/proxy/src/server/routes.rs` (pass `ttl_secs` into `CacheEntry` construction)
- Modify: `crates/proxy/src/server/chat_completions.rs` (same: pass `ttl_secs` into `CacheEntry`)
- Test: `crates/proxy/src/cache/memory.rs` (inline `#[cfg(test)]` module)

- [ ] **Step 1: Write the failing test for per-entry TTL**

Add to `crates/proxy/src/cache/memory.rs` in the `#[cfg(test)]` module:

```rust
#[tokio::test]
async fn per_entry_ttl_shorter_than_default() {
    // Cache default TTL is 10s, but we insert with 1s TTL.
    // Entry should expire after ~1s, not 10s.
    let config = CacheConfig {
        ttl_secs: 10,
        max_entries: 100,
        redis_url: None,
    };
    let cache = MemoryCache::new(&config);
    cache
        .put("test:short_ttl", test_entry("short"), 1)
        .await;

    // Present immediately
    assert!(cache.get("test:short_ttl").await.is_some());

    // Wait for the per-entry TTL (1s) plus margin
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(
        cache.get("test:short_ttl").await.is_none(),
        "entry with 1s TTL should have expired, but it survived (using global 10s TTL)"
    );
}

#[tokio::test]
async fn per_entry_ttl_longer_than_default() {
    // Cache default TTL is 1s, but we insert with 3s TTL.
    // Entry should survive past the default TTL.
    let config = CacheConfig {
        ttl_secs: 1,
        max_entries: 100,
        redis_url: None,
    };
    let cache = MemoryCache::new(&config);
    cache
        .put("test:long_ttl", test_entry("long"), 3)
        .await;

    // Wait past the default TTL (1s) but before the per-entry TTL (3s)
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(
        cache.get("test:long_ttl").await.is_some(),
        "entry with 3s TTL should still be alive at 1.5s, but it expired (using global 1s TTL)"
    );

    // Wait for the per-entry TTL to expire
    tokio::time::sleep(Duration::from_millis(2000)).await;
    assert!(cache.get("test:long_ttl").await.is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p anyllm_proxy per_entry_ttl -- --nocapture`
Expected: Both tests FAIL because `put()` ignores `ttl_secs` and uses the global TTL.

- [ ] **Step 3: Add `ttl_override` field to `CacheEntry`**

In `crates/proxy/src/cache/mod.rs`, add a field to `CacheEntry`:

```rust
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
```

- [ ] **Step 4: Fix all `CacheEntry` construction sites to include `ttl_secs`**

In `crates/proxy/src/server/routes.rs`, find `try_cache_response` (around line 500). Update the `CacheEntry` construction:

```rust
c.put(
    key,
    CacheEntry {
        response_body: resp_body,
        model,
        created_at: std::time::Instant::now(),
        ttl_secs: cache_ttl,
    },
    ttl,
)
.await;
```

In `crates/proxy/src/server/chat_completions.rs`, find the equivalent cache insertion and add `ttl_secs: cache_ttl`.

In `crates/proxy/src/cache/memory.rs` test helper `test_entry`, add `ttl_secs: None`.

In `crates/proxy/src/cache/redis.rs`, if `CacheEntry` is constructed there (on cache get), add `ttl_secs: None`.

Grep for `CacheEntry {` across the crate to find all construction sites.

- [ ] **Step 5: Implement moka `Expiry` trait in `memory.rs`**

Replace the entire `crates/proxy/src/cache/memory.rs` implementation (keep tests):

```rust
//! In-memory cache backend using moka's async cache with per-entry TTL.
//!
//! Uses moka's Expiry trait to support per-entry TTL overrides.
//! When a CacheEntry has ttl_secs set, that value is used instead of
//! the cache-level default.

use super::{CacheBackend, CacheConfig, CacheEntry};
use moka::Expiry;
use std::time::{Duration, Instant};

/// Per-entry TTL policy. Reads the ttl_secs field from CacheEntry
/// to determine expiration, falling back to the cache-level default.
struct EntryExpiry {
    default_ttl: Duration,
}

impl Expiry<String, CacheEntry> for EntryExpiry {
    /// Called after insert. Returns the TTL for this specific entry.
    fn expire_after_create(
        &self,
        _key: &String,
        value: &CacheEntry,
        _current_time: Instant,
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
/// overrides are applied at insert time via moka's Expiry trait.
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
        // Per-entry TTL is now handled by the Expiry trait reading entry.ttl_secs.
        // The _ttl_secs parameter is kept for CacheBackend trait compatibility
        // but the actual TTL comes from the entry itself.
        self.inner.insert(key.to_string(), entry).await;
    }
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p anyllm_proxy cache -- --nocapture`
Expected: All cache tests pass, including the two new per-entry TTL tests.

- [ ] **Step 7: Run full test suite to check for regressions**

Run: `cargo test -p anyllm_proxy`
Expected: All tests pass. No regressions from `CacheEntry` field addition.

- [ ] **Step 8: Commit**

```bash
git add crates/proxy/src/cache/mod.rs crates/proxy/src/cache/memory.rs crates/proxy/src/server/routes.rs crates/proxy/src/server/chat_completions.rs crates/proxy/src/cache/redis.rs
git commit -m "fix: enforce per-entry cache TTL via moka Expiry trait

Previously, MemoryCache.put() accepted ttl_secs but ignored it, using
only the global cache-level TTL. Now CacheEntry carries an optional
ttl_secs field, and a custom moka::Expiry implementation reads it to
set per-entry expiration. Redis backend already respected per-entry TTL."
```

---

## Task 2: Fix Stale Config Comment (Redis URL "Not Yet Used")

**Problem:** `CacheConfig.redis_url` has comment "Not yet used (placeholder for future)" but Redis cache and distributed rate limiting are fully implemented and operational. This creates operator confusion.

**Files:**
- Modify: `crates/proxy/src/cache/mod.rs:154`

- [ ] **Step 1: Read the current comment**

Verify the stale comment exists at `crates/proxy/src/cache/mod.rs:154`:
```rust
/// Optional Redis URL for L2 cache. Not yet used (placeholder for future).
pub redis_url: Option<String>,
```

- [ ] **Step 2: Fix the comment**

Replace the comment:

```rust
/// Optional Redis URL. Used by the Redis L2 cache backend (requires `redis` feature)
/// and distributed rate limiting. When set, responses are cached in Redis in addition
/// to the in-memory L1 cache, and rate limit state is shared across proxy instances.
pub redis_url: Option<String>,
```

- [ ] **Step 3: Run clippy to verify**

Run: `cargo clippy -p anyllm_proxy -- -D warnings`
Expected: Clean, no warnings.

- [ ] **Step 4: Commit**

```bash
git add crates/proxy/src/cache/mod.rs
git commit -m "docs: fix stale comment claiming redis_url is unused

Redis is actively used for L2 caching and distributed rate limiting.
The 'not yet used' comment was left over from initial scaffolding."
```

---

## Task 3: Add Streaming-Specific Metrics

**Problem:** Streaming requests only track total count (record_request) and success/error. No visibility into stream lifecycle: started, completed, client disconnect, upstream error, or abort. This is the most operationally important surface in a proxy.

**Fix:** Add streaming-specific atomic counters to the existing `Metrics` struct. Record outcomes from `StreamOutcome` already tracked in `streaming.rs`.

**Files:**
- Modify: `crates/proxy/src/metrics/mod.rs` (add streaming counters + snapshot fields)
- Modify: `crates/proxy/src/server/streaming.rs` (record streaming metrics from `StreamOutcome`)
- Modify: `crates/proxy/src/server/chat_completions.rs` (record streaming metrics for OpenAI path)
- Test: `crates/proxy/src/metrics/mod.rs` (inline tests)

- [ ] **Step 1: Write the failing test for streaming metrics**

Add to `crates/proxy/src/metrics/mod.rs` in the `#[cfg(test)]` module:

```rust
#[test]
fn streaming_metrics_counting() {
    let m = Metrics::new();
    m.record_stream_started();
    m.record_stream_started();
    m.record_stream_started();
    m.record_stream_completed();
    m.record_stream_failed();
    m.record_stream_client_disconnected();

    let s = m.snapshot();
    assert_eq!(s.streams_started, 3);
    assert_eq!(s.streams_completed, 1);
    assert_eq!(s.streams_failed, 1);
    assert_eq!(s.streams_client_disconnected, 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p anyllm_proxy streaming_metrics_counting`
Expected: FAIL with "no method named `record_stream_started`"

- [ ] **Step 3: Add streaming counters to Metrics**

In `crates/proxy/src/metrics/mod.rs`, add to `MetricsInner`:

```rust
#[derive(Debug, Default)]
struct MetricsInner {
    requests_total: AtomicU64,
    requests_success: AtomicU64,
    requests_error: AtomicU64,
    streams_started: AtomicU64,
    streams_completed: AtomicU64,
    streams_failed: AtomicU64,
    streams_client_disconnected: AtomicU64,
}
```

Add recording methods to `impl Metrics`:

```rust
/// A streaming request has begun sending SSE events to the client.
pub fn record_stream_started(&self) {
    self.inner.streams_started.fetch_add(1, Ordering::Relaxed);
}

/// A streaming request completed normally (backend sent [DONE]).
pub fn record_stream_completed(&self) {
    self.inner.streams_completed.fetch_add(1, Ordering::Relaxed);
}

/// A streaming request failed due to upstream error or buffer overflow.
pub fn record_stream_failed(&self) {
    self.inner.streams_failed.fetch_add(1, Ordering::Relaxed);
}

/// The client disconnected before the stream finished.
pub fn record_stream_client_disconnected(&self) {
    self.inner
        .streams_client_disconnected
        .fetch_add(1, Ordering::Relaxed);
}
```

Add to `snapshot()`:

```rust
pub fn snapshot(&self) -> MetricsSnapshot {
    MetricsSnapshot {
        requests_total: self.inner.requests_total.load(Ordering::Relaxed),
        requests_success: self.inner.requests_success.load(Ordering::Relaxed),
        requests_error: self.inner.requests_error.load(Ordering::Relaxed),
        streams_started: self.inner.streams_started.load(Ordering::Relaxed),
        streams_completed: self.inner.streams_completed.load(Ordering::Relaxed),
        streams_failed: self.inner.streams_failed.load(Ordering::Relaxed),
        streams_client_disconnected: self.inner.streams_client_disconnected.load(Ordering::Relaxed),
    }
}
```

Add to `MetricsSnapshot`:

```rust
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct MetricsSnapshot {
    pub requests_total: u64,
    pub requests_success: u64,
    pub requests_error: u64,
    /// Streaming sessions that began sending SSE events.
    pub streams_started: u64,
    /// Streaming sessions that completed normally.
    pub streams_completed: u64,
    /// Streaming sessions that failed (upstream error, buffer overflow).
    pub streams_failed: u64,
    /// Streaming sessions where the client disconnected early.
    pub streams_client_disconnected: u64,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p anyllm_proxy streaming_metrics_counting`
Expected: PASS

- [ ] **Step 5: Wire streaming metrics into `StreamOutcome::record()`**

In `crates/proxy/src/server/streaming.rs`, update `StreamOutcome::record()`:

```rust
impl StreamOutcome {
    /// Record metrics and return (HTTP status, error message) for logging.
    fn record(&self, metrics: &Metrics) -> (u16, Option<String>) {
        match self {
            Self::Completed => {
                metrics.record_success();
                metrics.record_stream_completed();
                (200, None)
            }
            Self::ClientDisconnected => {
                metrics.record_stream_client_disconnected();
                (499, Some("client disconnected".into()))
            }
            Self::UpstreamError => {
                // record_error() is already called at the point of failure
                metrics.record_stream_failed();
                (502, Some("stream interrupted".into()))
            }
        }
    }
}
```

Also, find where streaming is initiated in `streaming.rs` (where the SSE response is created and the background task is spawned) and add `metrics.record_stream_started()` at the point where we know the stream will begin. Look for where `StreamOutcome` is used and add the `record_stream_started()` call just before the SSE read loop begins. This should be at the top of the spawned task, before `read_sse_frames` is called.

- [ ] **Step 6: Wire streaming metrics into `chat_completions.rs` streaming path**

In `crates/proxy/src/server/chat_completions.rs`, find `chat_completions_stream()`. Add the same pattern:
- `metrics.record_stream_started()` at the top of the spawned stream task
- `metrics.record_stream_completed()` after successful stream completion
- `metrics.record_stream_failed()` on upstream error or buffer overflow
- `metrics.record_stream_client_disconnected()` when client disconnects (tx.send fails)

Look for the existing `metrics.record_success()` and `metrics.record_error()` calls and add the streaming-specific calls alongside them.

- [ ] **Step 7: Run full test suite**

Run: `cargo test -p anyllm_proxy`
Expected: All tests pass.

- [ ] **Step 8: Commit**

```bash
git add crates/proxy/src/metrics/mod.rs crates/proxy/src/server/streaming.rs crates/proxy/src/server/chat_completions.rs
git commit -m "feat: add streaming-specific metrics (started/completed/failed/disconnected)

Streaming requests now track lifecycle beyond just request count:
streams_started, streams_completed, streams_failed,
streams_client_disconnected. These are exposed in GET /metrics JSON
alongside the existing request counters."
```

---

## Task 4: Make Redis Rate Limit Failure Policy Configurable

**Problem:** `RedisRateLimiter` hardcodes fail-open on all Redis errors: `Ok(()) // Fail-open: allow the request`. Some operators need fail-closed or log-only behavior. This is a security-relevant policy decision that should not be hardcoded.

**Fix:** Add a `RateLimitFailPolicy` enum to config, thread it through to the rate limiter, and apply it on Redis errors.

**Files:**
- Modify: `crates/proxy/src/ratelimit.rs` (accept policy, apply on error)
- Modify: `crates/proxy/src/config/mod.rs` (add env var `RATE_LIMIT_FAIL_POLICY`)
- Modify: `crates/proxy/src/main.rs` (pass policy to RedisRateLimiter)
- Test: `crates/proxy/src/ratelimit.rs` (inline tests)

- [ ] **Step 1: Write the test for fail-closed behavior**

Add to `crates/proxy/src/ratelimit.rs` in the `#[cfg(test)]` module:

```rust
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
    // Default to open for unknown values
    assert!(matches!(
        RateLimitFailPolicy::from_env_str("unknown"),
        RateLimitFailPolicy::Open
    ));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p anyllm_proxy parse_rate_limit_fail_policy`
Expected: FAIL with "cannot find type `RateLimitFailPolicy`"

- [ ] **Step 3: Add `RateLimitFailPolicy` enum**

In `crates/proxy/src/ratelimit.rs`, add at the top (after the imports, before the `OnceLock`):

```rust
/// Policy for handling Redis rate limiter errors.
/// Controls whether requests are allowed or rejected when Redis is unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitFailPolicy {
    /// Allow requests when Redis is unavailable (default). Trades safety for availability.
    Open,
    /// Reject requests when Redis is unavailable. Trades availability for safety.
    Closed,
}

impl RateLimitFailPolicy {
    /// Parse from environment variable string. Defaults to Open for unrecognized values.
    pub fn from_env_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "closed" => Self::Closed,
            _ => Self::Open,
        }
    }

    /// Load from RATE_LIMIT_FAIL_POLICY env var. Defaults to Open.
    pub fn from_env() -> Self {
        std::env::var("RATE_LIMIT_FAIL_POLICY")
            .map(|v| Self::from_env_str(&v))
            .unwrap_or(Self::Open)
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p anyllm_proxy parse_rate_limit_fail_policy`
Expected: PASS

- [ ] **Step 5: Add policy field to `RedisRateLimiter` and apply on error**

In `crates/proxy/src/ratelimit.rs`, modify the `RedisRateLimiter` struct (inside `#[cfg(feature = "redis")]`):

```rust
#[cfg(feature = "redis")]
pub struct RedisRateLimiter {
    conn: ConnectionManager,
    fail_policy: RateLimitFailPolicy,
}
```

Update `new()`:

```rust
pub async fn new(redis_url: &str, fail_policy: RateLimitFailPolicy) -> Result<Self, redis::RedisError> {
    let client = redis::Client::open(redis_url)?;
    let conn = ConnectionManager::new(client).await?;
    Ok(Self { conn, fail_policy })
}
```

Update `check_rpm()` error handling:

```rust
pub async fn check_rpm(&self, key_hash_hex: &str, limit: u32, now_ms: u64) -> Result<(), u64> {
    let redis_key = format!("anyllm:rl:{key_hash_hex}:rpm");
    match self.check_rpm_inner(&redis_key, limit, now_ms).await {
        Ok(result) => result,
        Err(e) => {
            match self.fail_policy {
                RateLimitFailPolicy::Open => {
                    tracing::warn!(error = %e, "Redis RPM check failed, allowing request (fail-open)");
                    Ok(())
                }
                RateLimitFailPolicy::Closed => {
                    tracing::error!(error = %e, "Redis RPM check failed, rejecting request (fail-closed)");
                    Err(1) // retry-after 1 second
                }
            }
        }
    }
}
```

Apply the same pattern to `check_tpm()`.

- [ ] **Step 6: Update main.rs to pass policy**

In `crates/proxy/src/main.rs`, find the Redis rate limiter initialization (around line 96-110). Update:

```rust
#[cfg(feature = "redis")]
if let Ok(redis_url) = std::env::var("REDIS_URL") {
    let fail_policy = anyllm_proxy::ratelimit::RateLimitFailPolicy::from_env();
    match anyllm_proxy::ratelimit::RedisRateLimiter::new(&redis_url, fail_policy).await {
        Ok(limiter) => {
            tracing::info!(
                ?fail_policy,
                "Redis distributed rate limiting enabled"
            );
            anyllm_proxy::ratelimit::set_redis_rate_limiter(limiter);
        }
        Err(e) => {
            tracing::error!("Redis connection failed: {e}. Using local-only rate limiting.");
        }
    }
}
```

- [ ] **Step 7: Run full test suite**

Run: `cargo test -p anyllm_proxy`
Expected: All tests pass. The existing `get_redis_rate_limiter_returns_none_without_init` test still passes.

- [ ] **Step 8: Run clippy**

Run: `cargo clippy -p anyllm_proxy -- -D warnings`
Expected: Clean.

- [ ] **Step 9: Commit**

```bash
git add crates/proxy/src/ratelimit.rs crates/proxy/src/main.rs
git commit -m "feat: configurable Redis rate limit failure policy (open/closed)

Add RATE_LIMIT_FAIL_POLICY env var (default: open). When set to
'closed', Redis errors cause rate limit checks to reject requests
instead of allowing them. Operators who need strict rate enforcement
can now choose safety over availability."
```

---

## Task 5: Add Explicit Auth Mode Configuration

**Problem:** When OIDC is configured, JWT validation failure silently falls through to static/virtual key checks. This is intentional (virtual keys with dots) but creates auth-path ambiguity. Operators cannot restrict auth to JWT-only or keys-only. The fallthrough behavior is undocumented and can surprise security teams.

**Fix:** Add `AUTH_MODE` env var with three modes: `jwt_only`, `keys_only`, `jwt_or_keys` (default, current behavior). Log which auth path succeeded. Add EdDSA/OKP support to OIDC for broader IdP compatibility.

**Files:**
- Modify: `crates/proxy/src/server/middleware.rs` (add mode check, add auth-path logging)
- Modify: `crates/proxy/src/server/oidc.rs` (add EdDSA/OKP support in `parse_jwk`)
- Test: `crates/proxy/src/server/oidc.rs` (test EdDSA parsing)

- [ ] **Step 1: Write test for EdDSA/OKP JWK parsing**

Add to `crates/proxy/src/server/oidc.rs` in the `#[cfg(test)]` module:

```rust
#[test]
fn parse_eddsa_jwk() {
    let key = JwkKey {
        kid: Some("ed-key".to_string()),
        kty: "OKP".to_string(),
        alg: Some("EdDSA".to_string()),
        n: None,
        e: None,
        crv: Some("Ed25519".to_string()),
        // Valid base64url-encoded Ed25519 public key (32 bytes).
        x: Some("11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo".to_string()),
        y: None,
    };
    let entry = OidcConfig::parse_jwk(&key);
    assert!(entry.is_some(), "EdDSA/OKP keys should be supported");
    let entry = entry.unwrap();
    assert_eq!(entry.kid, "ed-key");
    assert!(matches!(entry.algorithm, Algorithm::EdDSA));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p anyllm_proxy parse_eddsa_jwk`
Expected: FAIL because `parse_jwk` returns `None` for `kty: "OKP"`.

- [ ] **Step 3: Add EdDSA support to `parse_jwk`**

In `crates/proxy/src/server/oidc.rs`, update `parse_jwk`:

```rust
fn parse_jwk(key: &JwkKey) -> Option<JwkEntry> {
    let kid = key.kid.clone().unwrap_or_default();
    let algorithm = match key.alg.as_deref() {
        Some("RS256") => Algorithm::RS256,
        Some("RS384") => Algorithm::RS384,
        Some("RS512") => Algorithm::RS512,
        Some("ES256") => Algorithm::ES256,
        Some("ES384") => Algorithm::ES384,
        Some("EdDSA") => Algorithm::EdDSA,
        // Default RSA keys without alg to RS256 (most common).
        None if key.kty == "RSA" => Algorithm::RS256,
        _ => return None,
    };

    let decoding_key = match key.kty.as_str() {
        "RSA" => {
            let n = key.n.as_ref()?;
            let e = key.e.as_ref()?;
            DecodingKey::from_rsa_components(n, e).ok()?
        }
        "EC" => {
            let x = key.x.as_ref()?;
            let y = key.y.as_ref()?;
            DecodingKey::from_ec_components(x, y).ok()?
        }
        "OKP" => {
            let x = key.x.as_ref()?;
            DecodingKey::from_ed_components(x).ok()?
        }
        _ => return None,
    };

    Some(JwkEntry {
        kid,
        algorithm,
        decoding_key,
    })
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p anyllm_proxy parse_eddsa_jwk`
Expected: PASS

- [ ] **Step 5: Also update the existing `parse_unknown_kty_returns_none` test**

The existing test expects OKP to return None. Update it to test a genuinely unsupported key type:

```rust
#[test]
fn parse_unknown_kty_returns_none() {
    let key = JwkKey {
        kid: Some("test".to_string()),
        kty: "oct".to_string(), // Symmetric key, not supported for JWT validation
        alg: Some("HS256".to_string()),
        n: None,
        e: None,
        crv: None,
        x: None,
        y: None,
    };
    assert!(OidcConfig::parse_jwk(&key).is_none());
}
```

- [ ] **Step 6: Write test for AuthMode parsing**

Add to `crates/proxy/src/server/middleware.rs` at the bottom of the file (or in a new `#[cfg(test)]` block):

```rust
#[cfg(test)]
mod auth_mode_tests {
    use super::*;

    #[test]
    fn parse_auth_mode() {
        assert!(matches!(
            AuthMode::from_env_str("jwt_only"),
            AuthMode::JwtOnly
        ));
        assert!(matches!(
            AuthMode::from_env_str("keys_only"),
            AuthMode::KeysOnly
        ));
        assert!(matches!(
            AuthMode::from_env_str("jwt_or_keys"),
            AuthMode::JwtOrKeys
        ));
        assert!(matches!(
            AuthMode::from_env_str("JWT_ONLY"),
            AuthMode::JwtOnly
        ));
        // Default to jwt_or_keys for unknown values
        assert!(matches!(
            AuthMode::from_env_str("unknown"),
            AuthMode::JwtOrKeys
        ));
    }
}
```

- [ ] **Step 7: Run test to verify it fails**

Run: `cargo test -p anyllm_proxy parse_auth_mode`
Expected: FAIL with "cannot find type `AuthMode`"

- [ ] **Step 8: Add `AuthMode` enum and integrate into `validate_auth`**

In `crates/proxy/src/server/middleware.rs`, add near the top (after the static declarations):

```rust
/// Controls which authentication paths are active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    /// Only accept JWT tokens. Static and virtual keys are rejected.
    JwtOnly,
    /// Only accept static and virtual API keys. JWTs are not checked.
    KeysOnly,
    /// Try JWT first, fall through to keys on failure (default, current behavior).
    JwtOrKeys,
}

impl AuthMode {
    pub fn from_env_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "jwt_only" => Self::JwtOnly,
            "keys_only" => Self::KeysOnly,
            _ => Self::JwtOrKeys,
        }
    }
}

static AUTH_MODE: LazyLock<AuthMode> = LazyLock::new(|| {
    let mode = std::env::var("AUTH_MODE")
        .map(|v| AuthMode::from_env_str(&v))
        .unwrap_or(AuthMode::JwtOrKeys);
    tracing::info!(?mode, "auth mode configured");
    mode
});
```

Then update `validate_auth` to respect the mode. Replace the OIDC check block (lines ~124-136):

```rust
    // Check 0: OIDC/JWT validation (if configured and mode allows it).
    if *AUTH_MODE != AuthMode::KeysOnly {
        if let Some(oidc) = OIDC_CONFIG.get() {
            if super::oidc::looks_like_jwt(&credential) {
                match oidc.validate_token(&credential) {
                    Ok(claims) => {
                        tracing::debug!(sub = ?claims.sub, auth_path = "jwt", "authentication successful");
                        request.extensions_mut().insert(claims);
                        return Ok(next.run(request).await);
                    }
                    Err(e) => {
                        if *AUTH_MODE == AuthMode::JwtOnly {
                            // In jwt_only mode, do not fall through to key checks.
                            tracing::debug!(error = %e, "JWT validation failed (jwt_only mode, no fallback)");
                            let err = create_anthropic_error(
                                anthropic::ErrorType::AuthenticationError,
                                "JWT validation failed.".to_string(),
                                None,
                            );
                            return Err((StatusCode::UNAUTHORIZED, Json(err)).into_response());
                        }
                        // In jwt_or_keys mode, fall through to key-based auth.
                        tracing::debug!(error = %e, "JWT validation failed, trying key-based auth");
                    }
                }
            } else if *AUTH_MODE == AuthMode::JwtOnly {
                // Credential doesn't look like JWT but mode requires it.
                let err = create_anthropic_error(
                    anthropic::ErrorType::AuthenticationError,
                    "JWT required but credential is not a valid JWT format.".to_string(),
                    None,
                );
                return Err((StatusCode::UNAUTHORIZED, Json(err)).into_response());
            }
        } else if *AUTH_MODE == AuthMode::JwtOnly {
            // No OIDC configured but jwt_only mode requested. Configuration error.
            tracing::error!("AUTH_MODE=jwt_only but OIDC_ISSUER_URL is not configured");
            let err = create_anthropic_error(
                anthropic::ErrorType::AuthenticationError,
                "Server misconfigured: JWT auth required but OIDC not configured.".to_string(),
                None,
            );
            return Err((StatusCode::UNAUTHORIZED, Json(err)).into_response());
        }
    }

    // In jwt_only mode, we should not reach here (handled above).
    // For keys_only and jwt_or_keys, continue with key-based auth.
```

Also add `auth_path = "static_key"` logging to the static key match:

```rust
    if env_key_match {
        tracing::debug!(auth_path = "static_key", "authentication successful");
        return Ok(next.run(request).await);
    }
```

And `auth_path = "virtual_key"` to the virtual key match (near line 276 where the return is):

```rust
    tracing::debug!(key_id = meta.id, auth_path = "virtual_key", "authentication successful");
```

- [ ] **Step 9: Run tests**

Run: `cargo test -p anyllm_proxy auth_mode`
Expected: PASS

Run: `cargo test -p anyllm_proxy`
Expected: All tests pass.

- [ ] **Step 10: Run clippy**

Run: `cargo clippy -p anyllm_proxy -- -D warnings`
Expected: Clean.

- [ ] **Step 11: Commit**

```bash
git add crates/proxy/src/server/middleware.rs crates/proxy/src/server/oidc.rs
git commit -m "feat: explicit auth modes (jwt_only/keys_only/jwt_or_keys) and EdDSA support

Add AUTH_MODE env var to control which authentication paths are active.
In jwt_only mode, failed JWT validation rejects immediately instead of
falling through to key checks. In keys_only mode, JWT validation is
skipped entirely. Default (jwt_or_keys) preserves existing behavior.

Also adds EdDSA/OKP JWK support for broader IdP compatibility (e.g.,
providers using Ed25519 signing keys).

Auth path is now logged on every successful authentication for
auditability."
```

---

## Task 6: Add Live Responses API Integration Tests

**Problem:** `OPENAI_API_FORMAT=responses` is wired up but not validated against the live API. This is a core API surface and a compatibility liability. The existing `live_api.rs` pattern shows how to structure these tests.

**Files:**
- Create: `crates/proxy/tests/live_responses.rs`
- Reference: `crates/proxy/tests/live_api.rs` (existing pattern to follow)

- [ ] **Step 1: Read the existing live_api.rs test structure**

Read `crates/proxy/tests/live_api.rs` to understand the test pattern: how the proxy is started, how requests are sent, and how assertions are structured.

- [ ] **Step 2: Create live_responses.rs test file**

Create `crates/proxy/tests/live_responses.rs`:

```rust
//! Live integration tests for the OpenAI Responses API backend.
//!
//! These tests are `#[ignore]` by default because they require a live OpenAI API key.
//! Run with:
//!   OPENAI_API_KEY=sk-... cargo test --test live_responses -- --ignored --test-threads=1
//!
//! The proxy must NOT be running separately; these tests start their own instance.

use reqwest::Client;
use serde_json::json;
use std::net::TcpListener;
use std::time::Duration;
use tokio::time::sleep;

/// Find an available port for the test proxy.
fn available_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind to random port");
    listener.local_addr().unwrap().port()
}

/// Start a proxy instance configured for Responses API format.
/// Returns the base URL (e.g., "http://127.0.0.1:12345").
async fn start_responses_proxy() -> String {
    let port = available_port();
    let base = format!("http://127.0.0.1:{port}");

    // Verify OPENAI_API_KEY is set
    let api_key = std::env::var("OPENAI_API_KEY")
        .expect("OPENAI_API_KEY must be set for live Responses API tests");

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            // Set env vars for this proxy instance
            std::env::set_var("LISTEN_PORT", port.to_string());
            std::env::set_var("OPENAI_API_FORMAT", "responses");
            std::env::set_var("PROXY_OPEN_RELAY", "true");
            std::env::set_var("OPENAI_API_KEY", &api_key);
            std::env::set_var("BACKEND", "openai");

            // Import and run the server (adjust to match actual server start function)
            // This depends on how the proxy exposes its server start function.
            // If the proxy doesn't expose a library function, use Command::new("cargo")
            // to start it as a subprocess instead.
            eprintln!("Test proxy starting on port {port} with Responses API format");

            // Fallback: start as subprocess
            let mut child = tokio::process::Command::new("cargo")
                .args(["run", "-p", "anyllm_proxy"])
                .env("LISTEN_PORT", port.to_string())
                .env("OPENAI_API_FORMAT", "responses")
                .env("PROXY_OPEN_RELAY", "true")
                .env("BACKEND", "openai")
                .spawn()
                .expect("failed to start proxy");

            child.wait().await.ok();
        });
    });

    // Wait for proxy to be ready
    let client = Client::new();
    for _ in 0..30 {
        sleep(Duration::from_millis(500)).await;
        if client.get(format!("{base}/health")).send().await.is_ok() {
            return base;
        }
    }
    panic!("Proxy did not start within 15 seconds");
}

#[tokio::test]
#[ignore]
async fn responses_api_non_streaming() {
    let base = start_responses_proxy().await;
    let client = Client::new();

    let body = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 50,
        "messages": [
            {"role": "user", "content": "Reply with exactly the word 'pong'. Nothing else."}
        ]
    });

    let resp = client
        .post(format!("{base}/v1/messages"))
        .header("x-api-key", "test-key")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .expect("request failed");

    let status = resp.status();
    let body: serde_json::Value = resp.json().await.expect("failed to parse response");

    assert_eq!(status, 200, "expected 200, got {status}: {body}");
    assert_eq!(body["type"], "message", "response type should be 'message'");
    assert!(body["content"].is_array(), "content should be an array");

    let text = body["content"][0]["text"]
        .as_str()
        .expect("first content block should have text");
    assert!(
        text.to_lowercase().contains("pong"),
        "expected 'pong' in response, got: {text}"
    );

    eprintln!("Responses API non-streaming test passed. Response: {body}");
}

#[tokio::test]
#[ignore]
async fn responses_api_streaming() {
    let base = start_responses_proxy().await;
    let client = Client::new();

    let body = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 50,
        "stream": true,
        "messages": [
            {"role": "user", "content": "Reply with exactly the word 'hello'. Nothing else."}
        ]
    });

    let resp = client
        .post(format!("{base}/v1/messages"))
        .header("x-api-key", "test-key")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .expect("request failed");

    let status = resp.status();
    assert_eq!(status, 200, "expected 200 for streaming response");

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/event-stream"),
        "expected SSE content type, got: {content_type}"
    );

    let body_text = resp.text().await.expect("failed to read SSE body");
    assert!(
        body_text.contains("event: message_start"),
        "SSE stream should contain message_start event"
    );
    assert!(
        body_text.contains("event: message_stop"),
        "SSE stream should contain message_stop event"
    );

    eprintln!("Responses API streaming test passed. Event count: {}", body_text.matches("event:").count());
}
```

**Important:** The exact subprocess management may need adjustment based on how `live_api.rs` handles it. Read that file first and match its pattern for starting and stopping the proxy.

- [ ] **Step 3: Verify the tests compile**

Run: `cargo test --test live_responses --no-run`
Expected: Compiles successfully.

- [ ] **Step 4: Run the tests (requires OPENAI_API_KEY)**

Run: `OPENAI_API_KEY=sk-... cargo test --test live_responses -- --ignored --test-threads=1`
Expected: Both tests pass if the Responses API backend is correctly wired.

If tests fail, document the specific failure mode (status code, error message, unexpected response shape) and create a follow-up fix. The test file itself is the deliverable for this task; fixing Responses API bugs is a separate task.

- [ ] **Step 5: Commit**

```bash
git add crates/proxy/tests/live_responses.rs
git commit -m "test: add live integration tests for OpenAI Responses API backend

Validates non-streaming and streaming through the Responses API format
(OPENAI_API_FORMAT=responses). Tests are #[ignore] by default; run with
OPENAI_API_KEY=sk-... cargo test --test live_responses -- --ignored"
```

---

## Summary

| Task | What | Files | Effort |
|------|------|-------|--------|
| 1 | Fix per-entry cache TTL | cache/mod.rs, cache/memory.rs, routes.rs, chat_completions.rs | M |
| 2 | Fix stale Redis comment | cache/mod.rs | S |
| 3 | Streaming metrics | metrics/mod.rs, streaming.rs, chat_completions.rs | M |
| 4 | Redis fail policy config | ratelimit.rs, config/mod.rs, main.rs | S |
| 5 | Auth modes + EdDSA | middleware.rs, oidc.rs | M |
| 6 | Live Responses API tests | tests/live_responses.rs | M |

After completing all 6 tasks, run the full verification:

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

All must pass with zero warnings before considering Phase 1 complete.
