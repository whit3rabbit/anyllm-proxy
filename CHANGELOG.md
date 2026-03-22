# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-03-21

Initial release of the Anthropic-to-OpenAI API translation proxy.

### Added

- **Core translation**: Anthropic Messages API requests translated to OpenAI Chat Completions format and back, with full round-trip fidelity for text, system prompts, stop reasons, temperature, and token limits.
- **Streaming SSE**: State machine in `streaming_map.rs` translates OpenAI chunk events into Anthropic SSE events (message_start, content_block_delta, message_stop). Bounded channel (32) for backpressure, client disconnect detection.
- **Tool calling**: Tool definitions (input_schema to function.parameters), tool_use/tool_result block translation, stateless ID passthrough, JSON string/object conversion between APIs.
- **File and document blocks**: Anthropic image content blocks translated to OpenAI image_url format. Document blocks (PDF) converted to text note fallback. 32MB size limit enforced.
- **Proxy server**: axum-based HTTP server with POST /v1/messages (streaming and non-streaming), GET /health, GET /metrics (JSON counters).
- **Middleware**: x-api-key and Authorization header auth validation, request ID injection, 32MB request size limit, concurrency limiting (API routes only), structured logging via tracing.
- **Compatibility endpoints**: GET /v1/models (static model list), POST /v1/messages/count_tokens (returns unsupported error), POST /v1/messages/batches (returns unsupported error).
- **Retry with backoff**: 3 retries with exponential backoff and 25% jitter for 429/5xx responses. Respects retry-after header. Applies to both non-streaming and streaming requests.
- **Security**: SSRF protection validates OPENAI_BASE_URL (rejects private IPs, loopback, cloud metadata endpoints, non-HTTP schemes). Header filtering and secret redaction in logs.
- **Observability**: Structured tracing logs, request ID correlation, GET /metrics endpoint with request count and success/error tracking.
- **Test suite**: 169 tests (135 translator unit, 21 proxy unit, 7 golden fixtures, 5 integration, 1 health check). Golden fixture files in `fixtures/` for round-trip validation.
- **CI**: GitHub Actions workflow (fmt, clippy, build, test).
- **Docker**: Multi-stage Dockerfile (rust builder, debian slim runtime).
- **Documentation**: README with quick start, configuration, endpoints, features, architecture, and limitations.

### Fixed

- Streaming retry: `chat_completion_stream()` now retries on 429/5xx with exponential backoff (was single-attempt).
- Streaming metrics: spawned task now records success/error counts (was request count only).
- Concurrency limit scope: moved to API routes only; health and metrics endpoints bypass the limit.

### Known Limitations

- OpenAI Responses API backend types are defined but not wired up; proxy only calls Chat Completions.
- Model name mapping is static/hardcoded (not configurable).
- Document blocks are converted to text notes (binary content not preserved through translation).
- Anthropic cache token fields are dropped on round-trip (no OpenAI equivalent).
- No live API integration tests (golden fixture tests only; live test requires OPENAI_API_KEY).
