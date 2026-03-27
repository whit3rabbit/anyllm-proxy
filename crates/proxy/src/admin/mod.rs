/// Token-based authentication for admin endpoints.
pub mod auth;
/// SQLite persistence for request logs and config overrides.
pub mod db;
/// Virtual API key generation, hashing, and rate limit state.
pub mod keys;
/// Admin HTTP router: config management, request log queries, metrics.
pub mod routes;
/// Per-key spend queries for cost tracking.
pub mod spend;
/// Shared mutable state between proxy handlers and admin server.
pub mod state;
/// WebSocket handler for live admin event streaming.
pub(crate) mod ws;
