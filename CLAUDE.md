# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

An Anthropic-to-OpenAI API translation proxy in Rust. Accepts Anthropic Messages API requests, translates them to OpenAI Chat Completions format, forwards to OpenAI, and translates back. Supports streaming SSE, tool calling, file/document blocks.

See PLAN.md for the full specification and TASKS.md for phased implementation status (all 11 phases complete).

## Current Status

**Working (verified):**
- Build: `cargo build` clean, `cargo clippy -- -D warnings` clean
- Tests: ~219 tests passing (158 translator, 61 proxy)
- Full Anthropic Messages API translation: non-streaming, streaming SSE, tool calling, file/document blocks
- Proxy middleware: health, auth, request ID, size limits, concurrency limits, retry with backoff
- Compatibility stubs: /v1/models, count_tokens, batches
- Model mapping and lossy-translation warnings

**Not implemented (types exist but not wired up):**
- OpenAI Responses API backend: `ResponsesRequest`/`ResponsesResponse` types are defined in `crates/translator/src/openai/responses.rs` but the proxy only calls Chat Completions. PLAN.md envisions runtime selection between the two.
- No live API integration tests (golden fixture tests only; live test requires OPENAI_API_KEY at runtime)
- Metrics endpoint exists (GET /metrics returns JSON counters) but streaming requests only track total count, not success/error

## Build and Test

```bash
cargo build                          # build everything
cargo test                           # run all tests (~219 tests)
cargo test -p anthropic_openai_translate  # translator crate only
cargo test -p anthropic_openai_proxy      # proxy crate only
cargo test health_endpoint            # single test by name
cargo clippy -- -D warnings          # lint
cargo fmt --check                    # format check
```

Run the proxy (requires OPENAI_API_KEY):
```bash
OPENAI_API_KEY=sk-... cargo run -p anthropic_openai_proxy
# Listens on 0.0.0.0:3000, health at GET /health
```

## Environment Variables

- `BACKEND`: Backend provider: `openai` (default) or `vertex`
- `OPENAI_API_KEY`: OpenAI API key (required when BACKEND=openai, empty default)
- `OPENAI_BASE_URL`: OpenAI base URL (default: `https://api.openai.com`)
- `LISTEN_PORT`: Server port (default: `3000`)
- `BIG_MODEL`: OpenAI model for sonnet/opus requests (default: `gpt-4o` for OpenAI, `gemini-2.5-pro` for Vertex)
- `SMALL_MODEL`: OpenAI model for haiku requests (default: `gpt-4o-mini` for OpenAI, `gemini-2.5-flash` for Vertex)
- `RUST_LOG`: Tracing filter (e.g., `info`, `anthropic_openai_proxy=debug`)
- `TLS_CLIENT_CERT_P12`: Path to PKCS#12 (.p12/.pfx) client certificate for mTLS to the backend (optional)
- `TLS_CLIENT_CERT_PASSWORD`: Password to decrypt the P12 file (required if P12 is set)
- `TLS_CA_CERT`: Path to PEM-encoded CA certificate for verifying the backend server (optional)
- `VERTEX_PROJECT`: GCP project ID (required when BACKEND=vertex)
- `VERTEX_REGION`: GCP region, e.g. `us-central1` (required when BACKEND=vertex)
- `VERTEX_API_KEY`: Google API key for Vertex AI (one of VERTEX_API_KEY or GOOGLE_ACCESS_TOKEN required when BACKEND=vertex)
- `GOOGLE_ACCESS_TOKEN`: OAuth bearer token for Vertex AI (alternative to VERTEX_API_KEY)

## Architecture

Cargo workspace with two crates:

### `crates/translator` (lib: `anthropic_openai_translate`)
Pure translation logic, no IO. Four modules:
- **`anthropic/`**: Anthropic Messages API types (request, response, streaming events, errors)
- **`openai/`**: OpenAI types for both Chat Completions and Responses APIs
- **`mapping/`**: Stateless conversion functions between the two APIs
  - `message_map`: Message/content block translation (system prompt -> developer role)
  - `tools_map`: Tool definitions and tool_use/tool_call translation
  - `usage_map`: Token usage field mapping
  - `errors_map`: HTTP status and error shape translation
  - `streaming_map`: SSE event stream translation state machine
- **`util/`**: JSON helpers, ID generation (uuid v4), secret redaction

### `crates/proxy` (bin: `anthropic_openai_proxy`)
HTTP proxy built on axum + reqwest:
- **`config.rs`**: Env-based configuration
- **`server/routes.rs`**: Axum router (POST /v1/messages, GET /health, GET /metrics, GET /v1/models, stubs for count_tokens and batches)
- **`server/middleware.rs`**: Auth validation (x-api-key), request ID injection, 32MB size limit, concurrency limit, logging
- **`server/sse.rs`**: SSE response helpers for Anthropic-format streaming
- **`backend/openai_client.rs`**: reqwest client calling OpenAI Chat Completions with retry/backoff on 429/5xx
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
- JSON fixtures in `fixtures/anthropic/` and `fixtures/openai/` are used for golden-file testing (4 fixture files).
- Retry logic: 3 retries with exponential backoff + 25% jitter, respects retry-after header.
- Backoff jitter is deterministic (upper bound, not random) to keep tests predictable.
- `ChatCompletionRequest` uses `#[serde(flatten)] pub extra: serde_json::Map` to capture unknown OpenAI fields (e.g., `seed`, `logprobs`, `logit_bias`, `n`, `reasoning_effort`). These pass through to OpenAI without typed handling. Only fields that require translation logic (not just forwarding) need explicit struct fields.

## Conventions

- Most source files reference their PLAN.md line ranges in a comment at the top.
- Test files live alongside source (`#[cfg(test)]` modules) and in `crates/proxy/tests/` for integration tests.
- Error types use `thiserror` derive macros.
- Test distribution: translator (~158 tests), proxy (~61 tests including integration/compatibility).

## References

- OpenAI API spec: https://github.com/openai/openai-openapi/blob/manual_spec/openapi.yaml (very large, ~70k+ lines). See https://simonwillison.net/2024/Dec/22/openai-openapi/ for context on the spec's size and structure. Do not attempt to load the full spec into context; reference specific sections as needed.
