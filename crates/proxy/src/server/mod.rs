/// Hop-by-hop headers that must not be forwarded to clients per RFC 7230.
pub(super) const HOP_BY_HOP: &[&str] = &[
    "transfer-encoding",
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "upgrade",
];

/// Audio transcription and text-to-speech passthrough handlers.
pub mod audio;
/// AWS Bedrock native endpoint handlers (Converse API + InvokeModel, SigV4 signing).
mod bedrock_native;
/// AWS Bedrock passthrough handler (SigV4 signing + event stream decoding).
mod bedrock_passthrough;
/// OpenAI Chat Completions input handler (POST /v1/chat/completions).
mod chat_completions;
/// Gemini native input handler (POST /v1beta/models/{model}:generateContent from gemini-cli).
pub mod gemini_input;
/// Gemini native generateContent handler (POST /v1/messages when GEMINI_API_FORMAT=native).
mod gemini_native;
/// Generic catch-all passthrough for any /v1/* path without an explicit handler.
mod generic_passthrough;
/// Image generation passthrough handler.
pub mod images;
/// Auth validation, request ID injection, size limits, concurrency limits, header logging.
pub mod middleware;
/// OIDC/JWT authentication (optional, enabled via OIDC_ISSUER_URL).
pub mod oidc;
/// Anthropic passthrough handler (no translation, forwards as-is).
mod passthrough;
/// Per-key request policy enforcement (model allowlists).
pub mod policy;
/// Axum router setup and request handlers for all API endpoints.
pub mod routes;
/// SSE response helpers for Anthropic-format streaming.
pub mod sse;
/// Shared state types for request handlers (AppState, AnthropicJson, ResolvedModel, etc.).
pub mod state;
/// SSE streaming handler with pre-stream error propagation and backpressure.
mod streaming;
/// Approximate token counting via tiktoken.
mod token_counting;
