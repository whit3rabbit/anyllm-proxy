# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

**anyllm-proxy** is an API translation proxy in Rust. Accepts Anthropic Messages API requests and OpenAI Chat Completions requests, translates between formats, forwards to any supported backend, and translates back. Supports streaming SSE, tool calling, file/document blocks, virtual key management, and optional OpenTelemetry export.

All implementation phases are complete.

## Current Status

**Working (verified):**
- Build: `cargo build` clean, `cargo clippy -- -D warnings` clean
- Tests: ~906 tests passing, 8 ignored (live API)
- Full Anthropic Messages API translation: non-streaming, streaming SSE, tool calling, file/document blocks
- `POST /v1/chat/completions` input: accepts OpenAI Chat Completions format, returns OpenAI format (unblocks all OpenAI-native clients)
- Azure OpenAI backend: `BACKEND=azure` with deployment-scoped URL and `api-key` header
- Virtual key management: admin API to create/list/revoke keys stored in SQLite, with DashMap cache for auth; no proxy restart required
- Per-key rate limiting: RPM/TPM sliding window per virtual key, returns 429 with `retry-after` on excess
- Rust client library v0.2.0: `ClientBuilder`, `ToolBuilder`, `messages_stream()` returning `impl Stream`
- Optional OpenTelemetry export: `--features otel` enables OTLP span export; zero overhead when feature is off
- Proxy middleware: health, auth (env-var keys + virtual keys), request ID, size limits, concurrency limits, retry with backoff
- Compatibility endpoints: /v1/models, count_tokens (approximate via tiktoken)
- Anthropic batch API: `/v1/messages/batches` (create, get, list, cancel, results) translated to/from OpenAI batch format
- Gemini native path: direct `generateContent` API, non-streaming + streaming SSE with full-response diffing
- Strict tool calling: sets `strict: true` on the forced tool when `tool_choice: {type: "tool", name: "X"}`
- Langfuse integration: native tracing when `LANGFUSE_PUBLIC_KEY` / `LANGFUSE_SECRET_KEY` set, or via config `callbacks: ["langfuse"]`
- CSRF protection: admin state-mutating endpoints require `X-CSRF-Token` header (double-submit cookie pattern)
- Per-entry cache TTL: `MemoryCache` enforces per-entry TTL via moka `Expiry` trait
- Configurable Redis fail policy: `RATE_LIMIT_FAIL_POLICY=open|closed` (default: open)
- Cost tracking: `record_cost()` wired into all paths; `key_id` + `cost_usd` in request log
- Audit log: admin config mutations recorded in SQLite `audit_log` table
- Spend alerts: webhook notifications at 80% / 95% / 100% of key budget
- Model allowlist: per-key policy with exact match and `prefix/*` wildcard
- Admin UI: login form (sessionStorage), virtual keys tab, models tab, request detail view, cost column, feed pause + filter
- Security hardening: plaintext HTTP startup warning, 1MB admin body limit, CSP header, model name validation
- Security fixes (2026-03-30 audit): `AWS_ACCESS_KEY_ID`/`GOOGLE_ACCESS_TOKEN` redacted in env endpoint; admin rate limiter uses sliding window; all audit entries include `source_ip`; OIDC discovery and webhook callbacks use SSRF-safe HTTP client and validate URLs against private IP ranges; CSRF public-route decision documented; non-Unix token file warning already present
- Model mapping and lossy-translation warnings
- `POST /v1/embeddings` passthrough: forwards directly to the backend with no translation; works with OpenAI, Vertex, Gemini (`gemini-embedding-exp-03-07`), and vLLM/HuggingFace models. Not mounted for the Anthropic passthrough backend.
- `x-anyllm-degradation` response header: set when features are silently dropped during translation (e.g., `top_k`, `thinking_config`, `cache_control`, `document_blocks`, `stop_sequences_truncated`)

**Not fully validated:**
- OpenAI Responses API backend: wired up via `OPENAI_API_FORMAT=responses` but not tested against live API
- AWS Bedrock backend: wired up via `BACKEND=bedrock` with SigV4 signing and Event Stream decoding; not tested against live API. Run with `AWS_REGION=... AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... cargo test --test live_bedrock -- --ignored --test-threads=1`
- Azure OpenAI backend: wired up via `BACKEND=azure`; not tested against live API. Run with `AZURE_OPENAI_API_KEY=... cargo test --test live_azure -- --ignored --test-threads=1`
- Live API integration tests exist (`crates/proxy/tests/live_api.rs`) but are `#[ignore]` by default; run with `OPENAI_API_KEY=sk-... cargo test --test live_api -- --ignored --test-threads=1`

## Build and Test

```bash
cargo build                          # build everything
cargo build --features otel          # with OpenTelemetry support
cargo test                           # run all tests (~906 tests, 8 ignored)
cargo test -p anyllm_client     # client crate only
cargo test -p anyllm_translate  # translator crate only
cargo test -p anyllm_proxy      # proxy crate only
cargo test health_endpoint            # single test by name
cargo test --test virtual_keys        # virtual key + rate limit integration tests
cargo clippy -- -D warnings          # lint
cargo fmt --check                    # format check
```

Run the proxy (requires OPENAI_API_KEY):
```bash
OPENAI_API_KEY=sk-... cargo run -p anyllm_proxy
# Listens on 0.0.0.0:3000, health at GET /health
```

## Environment Variables

- `BACKEND`: Backend provider: `openai` (default), `azure`, `vertex`, `gemini`, `anthropic` (passthrough), or `bedrock` (SigV4-signed, Anthropic format)
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
- `AZURE_OPENAI_ENDPOINT`: Azure OpenAI resource endpoint, e.g. `https://myresource.openai.azure.com` (required when BACKEND=azure)
- `AZURE_OPENAI_DEPLOYMENT`: Deployment name, e.g. `gpt4o` (required when BACKEND=azure)
- `AZURE_OPENAI_API_KEY`: Azure API key (required when BACKEND=azure)
- `AZURE_OPENAI_API_VERSION`: API version (default: `2024-10-21`, optional when BACKEND=azure)
- `PROXY_API_KEYS`: Comma-separated list of allowed API keys for proxy authentication (optional; if unset and PROXY_OPEN_RELAY is not set, all requests are rejected)
- `PROXY_OPEN_RELAY`: Set to `true` or `1` to accept any non-empty key (insecure, for local dev only)
- `LOG_BODIES`: Enable request/response body logging at debug level (`true` or `1`, default: disabled)
- `OTEL_EXPORTER_OTLP_ENDPOINT`: OTLP collector endpoint (default: `http://localhost:4318`). Only effective when built with `--features otel`.
- `OTEL_SERVICE_NAME`: Service name for exported traces. Only effective when built with `--features otel`.
- `OTEL_TRACES_SAMPLER`: Sampling strategy (default: `parentbased_always_on`). Only effective when built with `--features otel`.
- `PROXY_CONFIG`: Path to config file. TOML for multi-backend config, or `.yaml`/`.yml` for LiteLLM-compatible config with model_list routing.
- `IP_ALLOWLIST`: Comma-separated CIDR ranges for IP allowlisting (e.g., `192.168.1.0/24,10.0.0.0/8`). Bare IPs also accepted. When set, only matching IPs can access the proxy.
- `TRUST_PROXY_HEADERS`: Set to `true` or `1` to use `X-Forwarded-For` header for client IP when behind a reverse proxy. Only effective when `IP_ALLOWLIST` is set.
- `WEBHOOK_URLS`: Comma-separated webhook URLs for request completion notifications. Fire-and-forget HTTP POST with `RequestLogEntry` JSON payload.
- `RATE_LIMIT_FAIL_POLICY`: Behavior when Redis rate limiter is unavailable: `open` (default, allow requests) or `closed`/`deny` (reject with 503 and retry-after 60s).
- `REQUEST_TIMEOUT_SECS`: Maximum wall-clock seconds for a streaming response (default: 900, 0 = disabled). Prevents resource exhaustion from stalled backends.

### LiteLLM env var aliases

These LiteLLM env var names are accepted as aliases at startup (target takes precedence if already set):
- `LITELLM_MASTER_KEY` -> `PROXY_API_KEYS`
- `LITELLM_CONFIG` -> `PROXY_CONFIG`
- `AZURE_API_KEY` -> `AZURE_OPENAI_API_KEY`
- `AZURE_API_BASE` -> `AZURE_OPENAI_ENDPOINT`
- `AZURE_API_VERSION` -> `AZURE_OPENAI_API_VERSION`
- `AWS_REGION_NAME` -> `AWS_REGION`
- `LITELLM_IP_ALLOWLIST` -> `IP_ALLOWLIST`

## Architecture

Cargo workspace with three crates:

### `crates/client` (lib: `anyllm_client`) v0.2.0
High-level async HTTP client (Anthropic-in, Anthropic-out). Depends on `anyllm_translate` for translation logic. Key modules:
- **`client.rs`**: `Client` struct; `ClientBuilder` with method chaining (base_url, api_key, timeout, max_retries, tls_config); `messages()` for non-streaming, `messages_stream()` returning `impl Stream<Item = Result<StreamEvent, ClientError>>`
- **`tools.rs`**: `ToolBuilder` (name, description, input_schema) and `ToolChoiceBuilder` (auto/any/none/specific)
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
  - `message_map`: Message/content block translation (system prompt -> developer role); also `openai_to_anthropic_request` and `anthropic_to_openai_response` for reverse direction
  - `tools_map`: Tool definitions and tool_use/tool_call translation
  - `usage_map`: Token usage field mapping
  - `errors_map`: HTTP status and error shape translation
  - `streaming_map`: SSE event stream translation state machine (OpenAI chunks -> Anthropic events)
  - `reverse_streaming_map`: `ReverseStreamingTranslator` (Anthropic SSE events -> OpenAI ChatCompletionChunk)
  - `responses_message_map`: Anthropic to/from OpenAI Responses API mapping
  - `responses_streaming_map`: Responses API SSE event stream translation state machine
  - `warnings`: `TranslationWarnings` collector; lossy drops are surfaced via `x-anyllm-degradation` response header
- **`middleware/`**: Request/response handler orchestrating translation and backend calls
- **`util/`**: JSON helpers, ID generation (uuid v4), secret redaction
- **`config.rs`**: Translator-level configuration, **`error.rs`**: Error types, **`translate.rs`**: Top-level translation entry points

### `crates/proxy` (bin: `anyllm_proxy`)
HTTP proxy built on axum + reqwest:
- **`config/`**: Env-based configuration (`mod.rs`), TLS client cert setup (`tls.rs`), URL validation (`url_validation.rs`)
- **`server/routes.rs`**: Axum router (POST /v1/messages, POST /v1/chat/completions, GET /health, GET /metrics, GET /v1/models, stub for count_tokens, POST /v1/messages/batches and related batch endpoints); `record_vk_tpm` for post-response TPM recording
- **`server/chat_completions.rs`**: Handler for POST /v1/chat/completions (OpenAI format in, OpenAI format out); uses `ReverseStreamingTranslator` for streaming
- **`server/middleware.rs`**: Auth validation (env-var keys + virtual key DashMap), RPM/TPM pre-check, request ID injection, 32MB size limit, concurrency limit, `VirtualKeyContext` extension for TPM recording
- **`server/sse.rs`**: SSE response helpers for Anthropic-format streaming
- **`server/streaming.rs`**: SSE streaming handler with pre-stream error propagation and backpressure
- **`server/passthrough.rs`**: Anthropic passthrough handler (no translation, forwards as-is)
- **`server/bedrock_passthrough.rs`**: Bedrock handler (SigV4 signing, model-in-URL, Event Stream decoding for streaming)
- **`server/token_counting.rs`**: Approximate token counting via tiktoken
- **`backend/mod.rs`**: `BackendClient` enum (OpenAI/AzureOpenAI/OpenAIResponses/Vertex/GeminiOpenAI/Anthropic/Bedrock), `BackendError`, shared retry helpers
- **`backend/openai_client.rs`**: reqwest client calling OpenAI-compatible Chat Completions with retry/backoff on 429/5xx (used for OpenAI, Azure, Vertex, and Gemini backends)
- **`backend/anthropic_client.rs`**: Passthrough client forwarding Anthropic requests as-is to upstream Anthropic API (no translation)
- **`backend/bedrock_client.rs`**: AWS Bedrock client with SigV4 signing, AWS Event Stream binary frame decoder for streaming
- **`admin/`**: Admin server (localhost-only) with config management, WebSocket live updates (`ws.rs`), token auth (`auth.rs`, `db.rs`, `mod.rs`, `routes.rs`, `state.rs`)
- **`admin/keys.rs`**: Virtual key generation (SHA-256 hashed, `sk-vk` prefix), `VirtualKeyMeta`, `RateLimitState` (sliding window RPM/TPM)
- **`admin/routes.rs`**: Admin API endpoints including POST/GET/DELETE `/admin/api/keys` for virtual key CRUD
- **`admin-ui/`**: Static admin UI served by the admin server (`index.html`)
- **`metrics/`**: Request count, success/error tracking, exposed via GET /metrics
- **`otel.rs`**: OpenTelemetry initialization behind `#[cfg(feature = "otel")]`; `OtelGuard` shuts down the provider on drop

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
- Test distribution: translator (~305 tests including reverse translation), proxy + client (~240 tests including virtual key CRUD + rate limiting integration), plus doc tests. Counts shift as features are added.
- Virtual key CRUD integration tests are in `crates/proxy/tests/virtual_keys.rs`. They use a shared `OnceLock<DashMap>` to avoid fighting over the global `set_virtual_keys` OnceLock.
- The `PROXY_OPEN_RELAY=true` env var enables dev mode (any non-empty key accepted). Without it and without `PROXY_API_KEYS`, the proxy rejects all requests.

## References

- OpenAI API spec: https://github.com/openai/openai-openapi/blob/manual_spec/openapi.yaml (very large, ~70k+ lines). See https://simonwillison.net/2024/Dec/22/openai-openapi/ for context on the spec's size and structure. Do not attempt to load the full spec into context; reference specific sections as needed.

## Recent Changes
- 001-litellm-parity: Added Rust stable (1.83+, workspace edition 2021)
- 20260325-120000-litellm-gap-fill: Added POST /v1/chat/completions (OpenAI format input), Azure OpenAI backend (BACKEND=azure), virtual key management (admin API + DashMap cache), per-key RPM/TPM rate limiting, Rust client v0.2.0 (ClientBuilder + ToolBuilder + messages_stream), optional OpenTelemetry export (--features otel), ReverseStreamingTranslator in translator crate, reverse translation functions openai_to_anthropic_request / anthropic_to_openai_response
- parity-gaps: Routing strategies (least-busy, latency-based, weighted), dynamic model management admin API, /v1/models enrichment, IP allowlisting (CIDR, X-Forwarded-For), webhook callbacks
- 20260327: Gemini native generateContent path; Anthropic batch API (/v1/messages/batches); strict tool calling; Langfuse integration; CSRF protection; per-entry cache TTL; Redis fail policy; cost tracking + audit log + spend alerts + model allowlist; admin UI overhaul; security hardening; jsonwebtoken CVE fix

## Active Technologies
- Rust stable (1.83+, workspace edition 2021) (001-litellm-parity)
- SQLite (existing, extended with new tables); Redis (optional Tier 1 cache); Qdran (001-litellm-parity)
