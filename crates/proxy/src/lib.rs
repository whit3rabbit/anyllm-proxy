/// Admin server: localhost-only config management, request logging, WebSocket live updates.
pub mod admin;
/// Backend HTTP clients for OpenAI, Vertex, Gemini, and Anthropic passthrough.
pub mod backend;
/// Environment-based configuration, TLS client cert setup, URL validation.
pub mod config;
/// Request count, success/error tracking, exposed via GET /metrics.
pub mod metrics;
/// Axum HTTP server: routes, middleware (auth, request ID, size/concurrency limits), SSE streaming.
pub mod server;
