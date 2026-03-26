# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

**anyllm-proxy** is an Anthropic-to-OpenAI API translation proxy in Rust. Accepts Anthropic Messages API requests, translates them to OpenAI Chat Completions format, forwards to OpenAI, and translates back. Supports streaming SSE, tool calling, file/document blocks.

All 11 implementation phases are complete.

## Current Status

**Working (verified):**
- Build: `cargo build` clean, `cargo clippy -- -D warnings` clean
- Tests: ~480 tests passing, 4 ignored (live API)
- Full Anthropic Messages API translation: non-streaming, streaming SSE, tool calling, file/document blocks
- Proxy middleware: health, auth, request ID, size limits, concurrency limits, retry with backoff
- Compatibility endpoints: /v1/models, count_tokens (approximate via tiktoken), batches (stub)
- Model mapping and lossy-translation warnings
- `POST /v1/embeddings` passthrough: forwards directly to the backend with no translation; works with OpenAI, Vertex, Gemini (`gemini-embedding-exp-03-07`), and vLLM/HuggingFace models. Not mounted for the Anthropic passthrough backend.
- `x-anyllm-degradation` response header: set when features are silently dropped during translation (e.g., `top_k`, `thinking_config`, `cache_control`, `document_blocks`, `stop_sequences_truncated`)

**Not fully validated:**
- OpenAI Responses API backend: wired up via `OPENAI_API_FORMAT=responses` but not tested against live API
- AWS Bedrock backend: wired up via `BACKEND=bedrock` with SigV4 signing and Event Stream decoding; not tested against live API. Run with `AWS_REGION=... AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... cargo test --test live_bedrock -- --ignored --test-threads=1`
- Live API integration tests exist (`crates/proxy/tests/live_api.rs`) but are `#[ignore]` by default; run with `OPENAI_API_KEY=sk-... cargo test --test live_api -- --ignored --test-threads=1`
- Metrics endpoint exists (GET /metrics returns JSON counters) but streaming requests only track total count, not success/error

## Build and Test

```bash
cargo build                          # build everything
cargo test                           # run all tests (~480 tests, 4 ignored)
cargo test -p anyllm_client     # client crate only
cargo test -p anyllm_translate  # translator crate only
cargo test -p anyllm_proxy      # proxy crate only
cargo test health_endpoint            # single test by name
cargo clippy -- -D warnings          # lint
cargo fmt --check                    # format check
```

Run the proxy (requires OPENAI_API_KEY):
```bash
OPENAI_API_KEY=sk-... cargo run -p anyllm_proxy
# Listens on 0.0.0.0:3000, health at GET /health
```

## Environment Variables

- `BACKEND`: Backend provider: `openai` (default), `vertex`, `gemini`, `anthropic` (passthrough), or `bedrock` (SigV4-signed, Anthropic format)
- `OPENAI_API_KEY`: OpenAI API key (required when BACKEND=openai, empty default)
- `OPENAI_BASE_URL`: OpenAI base URL (default: `https://api.openai.com`)
- `OPENAI_API_FORMAT`: OpenAI API format: `chat` (default, Chat Completions) or `responses` (Responses API). Only relevant when BACKEND=openai.
- `LISTEN_PORT`: Server port (default: `3000`)
- `BIG_MODEL`: Backend model for sonnet/opus requests (default: `gpt-4o` for OpenAI, `gemini-2.5-pro` for Vertex/Gemini)
- `SMALL_MODEL`: Backend model for haiku requests (default: `gpt-4o-mini` for OpenAI, `gemini-2.5-flash` for Vertex/Gemini)
- `RUST_LOG`: Tracing filter (e.g., `info`, `anyllm_proxy=debug`)
- `TLS_CLIENT_CERT_P12`: Path to PKCS#12 (.p12/.pfx) client certificate for mTLS to the backend (optional)
- `TLS_CLIENT_CERT_PASSWORD`: Password to decrypt the P12 file (required if P12 is set)
- `TLS_CA_CERT`: Path to PEM-encoded CA certificate for verifying the backend server (optional)
- `VERTEX_PROJECT`: GCP project ID (required when BACKEND=vertex)
- `VERTEX_REGION`: GCP region, e.g. `us-central1` (required when BACKEND=vertex)
- `VERTEX_API_KEY`: Google API key for Vertex AI (one of VERTEX_API_KEY or GOOGLE_ACCESS_TOKEN required when BACKEND=vertex)
- `GOOGLE_ACCESS_TOKEN`: OAuth bearer token for Vertex AI (alternative to VERTEX_API_KEY)
- `GEMINI_API_KEY`: Google API key for Gemini Developer API (required when BACKEND=gemini)
- `GEMINI_BASE_URL`: Gemini API base URL (default: `https://generativelanguage.googleapis.com/v1beta`)
- `AWS_REGION`: AWS region for Bedrock (required when BACKEND=bedrock)
- `AWS_ACCESS_KEY_ID`: AWS access key ID for SigV4 signing (required when BACKEND=bedrock)
- `AWS_SECRET_ACCESS_KEY`: AWS secret access key for SigV4 signing (required when BACKEND=bedrock)
- `AWS_SESSION_TOKEN`: Temporary session token for STS credentials (optional, BACKEND=bedrock)
- `PROXY_API_KEYS`: Comma-separated list of allowed API keys for proxy authentication (optional; if unset, any non-empty key is accepted)
- `LOG_BODIES`: Enable request/response body logging at debug level (`true` or `1`, default: disabled)
- `OTEL_EXPORTER_OTLP_ENDPOINT`: OTLP collector endpoint (default: `http://localhost:4318`). Only effective when built with `--features otel`.
- `OTEL_SERVICE_NAME`: Service name for exported traces. Only effective when built with `--features otel`.
- `OTEL_TRACES_SAMPLER`: Sampling strategy (default: `parentbased_always_on`). Only effective when built with `--features otel`.

## Architecture

Cargo workspace with three crates:

### `crates/client` (lib: `anyllm_client`)
High-level async HTTP client (Anthropic-in, Anthropic-out). Depends on `anyllm_translate` for translation logic. Key modules:
- **`client.rs`**: `Client` struct with `ClientConfig` builder; `messages()` for non-streaming, streaming variant for SSE
- **`http.rs`**: reqwest client builder with optional SSRF-safe DNS resolution and mTLS (PKCS#12)
- **`retry.rs`**: Generic retry with exponential backoff + jitter; `is_retryable`, `send_with_retry`
- **`rate_limit.rs`**: Parses `x-ratelimit-*` / `retry-after` headers into a typed struct
- **`sse.rs`**: Framework-agnostic SSE frame parser (`find_double_newline`)
- **`error.rs`**: `ClientError` enum

### `crates/translator` (lib: `anyllm_translate`)
Pure translation logic, no IO. Key modules:
- **`anthropic/`**: Anthropic Messages API types (request, response, streaming events, errors)
- **`openai/`**: OpenAI types for both Chat Completions and Responses APIs
- **`mapping/`**: Stateless conversion functions between the two APIs
  - `message_map`: Message/content block translation (system prompt -> developer role)
  - `tools_map`: Tool definitions and tool_use/tool_call translation
  - `usage_map`: Token usage field mapping
  - `errors_map`: HTTP status and error shape translation
  - `streaming_map`: SSE event stream translation state machine
  - `responses_message_map`: Anthropic to/from OpenAI Responses API mapping
  - `responses_streaming_map`: Responses API SSE event stream translation state machine
  - `warnings`: `TranslationWarnings` collector; lossy drops are surfaced via `x-anyllm-degradation` response header
- **`middleware/`**: Request/response handler orchestrating translation and backend calls
- **`util/`**: JSON helpers, ID generation (uuid v4), secret redaction
- **`config.rs`**: Translator-level configuration, **`error.rs`**: Error types, **`translate.rs`**: Top-level translation entry points

### `crates/proxy` (bin: `anyllm_proxy`)
HTTP proxy built on axum + reqwest:
- **`config/`**: Env-based configuration (`mod.rs`), TLS client cert setup (`tls.rs`), URL validation (`url_validation.rs`)
- **`server/routes.rs`**: Axum router (POST /v1/messages, GET /health, GET /metrics, GET /v1/models, stubs for count_tokens and batches)
- **`server/middleware.rs`**: Auth validation (x-api-key), request ID injection, 32MB size limit, concurrency limit, logging
- **`server/sse.rs`**: SSE response helpers for Anthropic-format streaming
- **`server/streaming.rs`**: SSE streaming handler with pre-stream error propagation and backpressure
- **`server/passthrough.rs`**: Anthropic passthrough handler (no translation, forwards as-is)
- **`server/bedrock_passthrough.rs`**: Bedrock handler (SigV4 signing, model-in-URL, Event Stream decoding for streaming)
- **`server/token_counting.rs`**: Approximate token counting via tiktoken
- **`backend/mod.rs`**: `BackendClient` enum (OpenAI/OpenAIResponses/Vertex/GeminiOpenAI/Anthropic/Bedrock), `BackendError`, shared retry helpers
- **`backend/openai_client.rs`**: reqwest client calling OpenAI-compatible Chat Completions with retry/backoff on 429/5xx (used for OpenAI, Vertex, and Gemini backends)
- **`backend/anthropic_client.rs`**: Passthrough client forwarding Anthropic requests as-is to upstream Anthropic API (no translation)
- **`backend/bedrock_client.rs`**: AWS Bedrock client with SigV4 signing, AWS Event Stream binary frame decoder for streaming
- **`admin/`**: Admin server (localhost-only) with config management, WebSocket live updates (`ws.rs`), token auth (`auth.rs`, `db.rs`, `mod.rs`, `routes.rs`, `state.rs`)
- **`admin-ui/`**: Static admin UI served by the admin server (`index.html`)
- **`metrics/`**: Request count, success/error tracking, exposed via GET /metrics

### Data Flow
```
Client (Anthropic format) -> proxy (axum)
  -> translator: anthropic types -> mapping -> openai types
  -> backend: reqwest -> OpenAI Chat Completions
  -> translator: openai types -> mapping -> anthropic types
  -> proxy (axum) -> Client (Anthropic format)
```

## Key Design Decisions

- The translator crate is deliberately IO-free: all mapping is pure `fn(A) -> B`. This makes it testable without mocks.
- Tool call IDs pass through directly (Anthropic tool_use.id = OpenAI tool_call.id).
- OpenAI `arguments` is a JSON string; Anthropic `input` is a JSON object. The mapping layer handles serialization.
- Streaming uses a state machine in `streaming_map.rs` that transforms OpenAI chunk events into Anthropic SSE events, with bounded channel (32) for backpressure.
- JSON fixtures in `fixtures/anthropic/` and `fixtures/openai/` are used for golden-file testing (14 fixture files).
- Retry logic: 3 retries with exponential backoff + 25% jitter, respects retry-after header.
- Backoff jitter is deterministic (upper bound, not random) to keep tests predictable.
- `ChatCompletionRequest` uses `#[serde(flatten)] pub extra: serde_json::Map` to capture unknown OpenAI fields (e.g., `seed`, `logprobs`, `logit_bias`, `n`, `reasoning_effort`). These pass through to OpenAI without typed handling. Only fields that require translation logic (not just forwarding) need explicit struct fields.
- DeepSeek/Qwen thinking model support: `reasoning_content` on `ChatMessage` and `ChunkDelta` maps bidirectionally to Anthropic thinking blocks. Request direction: Anthropic `Thinking` content blocks become `reasoning_content` on the assistant message. Response direction: `reasoning_content` becomes an Anthropic `Thinking` block preceding the text content. Streaming: `reasoning_content` deltas open a thinking content block, which is closed when regular `content` deltas begin. The `thinking` config (`budget_tokens`) is stripped with a warning since it has no standard OpenAI equivalent.
- Local LLM compatibility: streaming tool calls handle missing/empty IDs by generating synthetic `toolu_` IDs. `FinishReason::Unknown` (serde catch-all) maps to `end_turn` for providers like DeepSeek that use non-standard finish reasons (e.g., `insufficient_system_resource`).

## Conventions

- Some source files reference PLAN.md line ranges in a comment at the top (historical; PLAN.md has been removed).
- Test files live alongside source (`#[cfg(test)]` modules) and in `crates/proxy/tests/` for integration tests.
- Error types use `thiserror` derive macros.
- Test distribution: translator (~273 tests), proxy + client (~30 tests including integration/compatibility). Counts shift as features are added.

## References

- OpenAI API spec: https://github.com/openai/openai-openapi/blob/manual_spec/openapi.yaml (very large, ~70k+ lines). See https://simonwillison.net/2024/Dec/22/openai-openapi/ for context on the spec's size and structure. Do not attempt to load the full spec into context; reference specific sections as needed.

## Recent Changes
- 20260325-120000-litellm-gap-fill: Added [if applicable, e.g., PostgreSQL, CoreData, files or N/A]
