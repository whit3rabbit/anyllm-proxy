//! Redis cache backend (future implementation).
//!
//! This module is a placeholder for an L2 cache backed by Redis.
//! It will be implemented behind a `redis` feature flag when the
//! `redis` crate dependency is added.
//!
//! Design notes:
//! - Will use `redis::aio::ConnectionManager` for async, pooled connections.
//! - CacheEntry will be serialized via serde_json for storage.
//! - Graceful fallback: if Redis is unreachable, log error and continue
//!   (memory cache still serves as L1).
//! - TTL is set via Redis SETEX/PSETEX, giving true per-entry expiration.
