/// AWS Bedrock passthrough handler (SigV4 signing + event stream decoding).
mod bedrock_passthrough;
/// OpenAI Chat Completions input handler (POST /v1/chat/completions).
mod chat_completions;
/// Auth validation, request ID injection, size limits, concurrency limits, header logging.
pub mod middleware;
/// Anthropic passthrough handler (no translation, forwards as-is).
mod passthrough;
/// Axum router setup and request handlers for all API endpoints.
pub mod routes;
/// SSE response helpers for Anthropic-format streaming.
pub mod sse;
/// SSE streaming handler with pre-stream error propagation and backpressure.
mod streaming;
/// Approximate token counting via tiktoken.
mod token_counting;
