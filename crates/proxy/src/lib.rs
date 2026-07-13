/// Admin server: localhost-only config management, request logging, WebSocket live updates.
pub mod admin;
/// Backend HTTP clients for OpenAI, Vertex, Gemini, and Anthropic passthrough.
pub mod backend;
/// Async batch job submission and management (US3).
pub mod batch;
/// Response caching with in-memory (moka) and optional Redis tier (US1).
pub mod cache;
/// Webhook callback support for request completion notifications.
pub mod callbacks;
/// Environment-based configuration, TLS client cert setup, URL validation.
pub mod config;
/// Per-request cost tracking and model pricing (US4).
pub mod cost;
/// Pure env-file parser (no I/O, no set_var). Used by startup bootstrap and admin import endpoint.
pub mod env_parser;
/// Backend fallback chains for transparent failover (US2).
pub mod fallback;
/// Named integration registry (Langfuse, etc.).
pub mod integrations;
/// Request count, success/error tracking, exposed via GET /metrics.
pub mod metrics;
/// Provider/model-specific OpenAI-compatible tool request and response normalization.
pub mod openai_tool_policy;
/// Opt-in prompt compression (Frozen-Frontier Extractive Compression / FFEC) shim.
pub mod optimizer;
/// Optional OpenTelemetry OTLP trace export (requires `otel` feature).
#[cfg(feature = "otel")]
pub mod otel;
/// Opt-in text-to-image context compression (Anthropic-passthrough only).
pub mod pxpipe;
/// Distributed rate limiting via Redis sorted sets (requires `redis` feature).
pub mod ratelimit;
/// Opt-in command-aware tool-output compression (RTK port).
pub mod rtk;
/// In-process chat completion runtime without HTTP route ownership.
pub mod runtime;
/// Axum HTTP server: routes, middleware (auth, request ID, size/concurrency limits), SSE streaming.
pub mod server;
/// Anthropic thinking-block record-and-restore repair (Anthropic-passthrough only).
pub mod thinking_repair;
/// Optional built-in server-side tools and registry.
pub mod tools;
