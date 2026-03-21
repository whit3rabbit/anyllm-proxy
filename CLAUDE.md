# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

An Anthropic-to-OpenAI API translation proxy in Rust. Accepts Anthropic Messages API requests, translates them to OpenAI Chat Completions format, forwards to OpenAI, and translates back. Supports streaming SSE, tool calling, file/document blocks.

See PLAN.md for the full specification and TASKS.md for phased implementation status (all 11 phases complete).

## Current Status

**Working (verified):**
- Build: `cargo build` clean, `cargo clippy -- -D warnings` clean
- Tests: 169 tests passing (135 translator, 21 proxy unit, 7 golden fixtures, 5 integration, 1 health)
- Non-streaming translation: Anthropic Messages -> OpenAI Chat Completions, round-trip
- Streaming SSE: state machine translates OpenAI chunks -> Anthropic stream events
- Tool calling: tool definitions, tool_use/tool_result, ID passthrough, JSON string/object conversion
- File/document blocks: image and PDF base64 translation with size limits
- Proxy: health, auth, request ID, size limits, concurrency limits, retry with backoff
- Compatibility stubs: /v1/models (static list), /v1/messages/count_tokens and /v1/messages/batches (return unsupported error)

**Not implemented (types exist but not wired up):**
- OpenAI Responses API backend: `ResponsesRequest`/`ResponsesResponse` types are defined in `crates/translator/src/openai/responses.rs` but the proxy only calls Chat Completions. PLAN.md envisions runtime selection between the two.
- No live API integration tests (golden fixture tests only; live test requires OPENAI_API_KEY at runtime)
- No metrics endpoint exposed (metrics struct exists in `crates/proxy/src/metrics/` but no GET /metrics route)
- Model name mapping is static (hardcoded claude model list in routes.rs), not configurable

## Build and Test

```bash
cargo build                          # build everything
cargo test                           # run all tests (169 tests)
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

- `OPENAI_API_KEY`: OpenAI API key (required for proxying, empty default)
- `OPENAI_BASE_URL`: OpenAI base URL (default: `https://api.openai.com`)
- `LISTEN_PORT`: Server port (default: `3000`)
- `RUST_LOG`: Tracing filter (e.g., `info`, `anthropic_openai_proxy=debug`)

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
- **`server/routes.rs`**: Axum router (POST /v1/messages, GET /health, GET /v1/models, stubs for count_tokens and batches)
- **`server/middleware.rs`**: Auth validation (x-api-key), request ID injection, 32MB size limit, concurrency limit, logging
- **`server/sse.rs`**: SSE response helpers for Anthropic-format streaming
- **`backend/openai_client.rs`**: reqwest client calling OpenAI Chat Completions with retry/backoff on 429/5xx
- **`metrics/`**: Request count, latency, error rate tracking (internal only, no endpoint)

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

## Conventions

- Most source files reference their PLAN.md line ranges in a comment at the top.
- Test files live alongside source (`#[cfg(test)]` modules) and in `crates/proxy/tests/` for integration tests.
- Error types use `thiserror` derive macros.
- Test distribution: translator has bulk of tests (135), proxy tests focus on SSE formatting (11) and client retry logic (8).
