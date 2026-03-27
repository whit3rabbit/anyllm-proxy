/// Admin server: localhost-only config management, request logging, WebSocket live updates.
pub mod admin;
/// Backend HTTP clients for OpenAI, Vertex, Gemini, and Anthropic passthrough.
pub mod backend;
/// Async batch job submission and management (US3).
pub mod batch;
/// Response caching with in-memory (moka) and optional Redis tier (US1).
pub mod cache;
/// Environment-based configuration, TLS client cert setup, URL validation.
pub mod config;
/// Per-request cost tracking and model pricing (US4).
pub mod cost;
/// Backend fallback chains for transparent failover (US2).
pub mod fallback;
/// Request count, success/error tracking, exposed via GET /metrics.
pub mod metrics;
/// Optional OpenTelemetry OTLP trace export (requires `otel` feature).
#[cfg(feature = "otel")]
pub mod otel;
/// Axum HTTP server: routes, middleware (auth, request ID, size/concurrency limits), SSE streaming.
pub mod server;
