# Tasks: Anthropic-to-OpenAI Translation Proxy

Reference: [PLAN.md](./PLAN.md)

## Dependency Graph

```
P1 ──┬── P2 ──┬── P4 ──┬── P5 ──┬── P7 ──┐
     └── P3 ──┘        └── P6 ──┤        ├── P10 ── P11 ── P12 ── P13 ── P14
                                ├── P8 ──┤
                                └── P9 ──┘

P14 ── P14b (transparent proxy) ── P15 (model mapping)
P14 ──┬── P16 (max_completion_tokens)
      ├── P17 (extended thinking) ── depends on P15 for model-aware behavior
      ├── P18 (top_k)
      ├── P19 (token counting)
      ├── P20 (gemini research, complete)
      └── P21 (library mode) ── depends on P15, P16, P17 for complete translation

P20 + P15 ── P20a (Vertex OpenAI-compat) ── P20b (backend abstraction)
P20 ── P20c (Gemini types) ── P20d (schema sanitizer)
P20c + P20d ── P20e (Gemini mapping) ── P20f (Gemini streaming)
P20b + P20f ── P20g (native Gemini client)
```

## Phase 1: Project Scaffolding
**PLAN.md lines 606-665, 575-587**

- [x] Cargo workspace: `translator` (lib) + `proxy` (bin)
- [x] Dependencies: tokio, axum, reqwest, serde, serde_json
- [x] Directory structure per PLAN.md tree
- [x] Research: document dependency versions in `docs/dependencies.md`
- [x] Test: health endpoint returns 200, `cargo test` passes

## Phase 2: Anthropic Domain Types
**PLAN.md lines 64-76, 156-163, 667-697**

- [x] `AnthropicMessageCreateRequest` and response structs
- [x] Content block types: text, image, document, tool_use, tool_result
- [x] Error types and header parsing (x-api-key, anthropic-version)
- [x] Research: verify structs against current Anthropic API docs -> `docs/api-diffs-anthropic.md`
- [x] Test: deserialization, rejection of malformed requests, round-trip serde

## Phase 3: OpenAI Domain Types
**PLAN.md lines 78-94, 165-171, 700-725**

- [x] `OpenAIChatCompletionRequest/Response` structs
- [x] `OpenAIResponsesRequest/Response` structs
- [x] Error types, rate limit headers, tool types
- [x] Research: verify structs against current OpenAI API docs -> `docs/api-diffs-openai.md`
- [x] Test: deserialization, tool call response parsing, round-trip serde

## Phase 4: Non-Streaming Message Translation
**PLAN.md lines 765-793, 964-977**

- [x] message_map: Anthropic messages -> OpenAI messages (system -> developer role)
- [x] tools_map: input_schema -> function.parameters, tool_choice mapping
- [x] usage_map: prompt_tokens -> input_tokens, completion_tokens -> output_tokens
- [x] errors_map: HTTP status codes, stop_reason mapping
- [x] Research: comprehensive field mapping table -> `docs/field-mapping.md`
- [x] Test: basic text, system prompt, tool defs, stop reasons, temperature clamping

## Phase 5: Tool Calling Translation
**PLAN.md lines 103-121, 770-776, 273-386**

- [x] Stateless ID bridge: OpenAI tool_call.id = Anthropic tool_use.id
- [x] Conversation history walker: tool_use -> tool_calls, tool_result -> tool messages
- [x] JSON string (OpenAI arguments) vs JSON object (Anthropic input) handling
- [x] Research: exact tool schemas from both APIs -> `docs/tool-calling-diffs.md`
- [x] Test: single/multi tool calls, ID preservation, invalid JSON, multi-turn

## Phase 6: Proxy Server and Routing
**PLAN.md lines 565-566, 981-997, 890-893**

- [x] Config: OpenAI key, base URL, port (model list is static/hardcoded, not configurable)
- [x] Routes: POST /v1/messages (non-streaming)
- [x] Middleware: auth, request ID, 32MB size limit, logging
- [x] OpenAI client: reqwest calling Chat Completions
- [x] Research: header rules, error shapes, rate limit mapping -> `docs/proxy-architecture.md`
- [x] Test: happy path, auth failure, size limit, upstream errors, model aliases

## Phase 7: Streaming SSE Translation
**PLAN.md lines 123-151, 387-432, 796-807**

- [x] Anthropic SSE emitter (message_start -> deltas -> message_stop)
- [x] OpenAI ChatCompletions chunk parser (Responses event parser not implemented)
- [x] Streaming state machine in streaming_map.rs
- [x] Backpressure via bounded channel, client disconnect handling
- [x] Research: full event sequences for both APIs -> `docs/streaming-diffs.md`
- [x] Test: text streaming, event ordering, tool streaming, disconnect, golden fixtures

## Phase 8: Files and Document Blocks
**PLAN.md lines 177-191, 856-859**

- [x] Anthropic image content blocks -> OpenAI image_url; document blocks -> text note fallback
- [x] Size limit enforcement (32MB)
- [x] Research: file/image format support comparison -> `docs/file-handling-diffs.md`
- [x] Test: PDF base64, image base64/URL, oversized rejection, mixed content

## Phase 9: Compatibility Endpoints
**PLAN.md lines 860-865, 951-962**

- [x] GET /v1/models (proxy or static mapping)
- [x] POST /v1/messages/count_tokens (local approx or unsupported error)
- [x] POST /v1/messages/batches (unsupported error)
- [x] Research: full endpoint list, model name mapping -> `docs/compatibility-contract.md`
- [x] Test: models list, count_tokens, batches rejection, unknown routes

## Phase 10: Hardening, Security, Observability
**PLAN.md lines 867-870, 889-910**

- [x] Retry/backoff for 429/5xx with retry-after
- [x] Header filtering, secret redaction, request ID correlation
- [x] Structured logging (tracing)
- [x] Metrics endpoint (GET /metrics returns JSON counters, wired into messages handler)
- [x] SSRF protection (validate_base_url rejects private IPs, loopback, cloud metadata, non-http schemes)
- [x] Concurrency limits
- [x] Research: security audit -> `docs/security-audit.md`
- [x] Test: retry, header redaction, concurrency limit, clippy
- [x] Test: SSRF block (unit tests in config.rs + integration tests in compatibility.rs)

## Phase 11: End-to-End Validation
**PLAN.md full document**

- [x] Full test suite green
- [x] Fixture validation against current API docs
- [x] Compatibility checklist (PLAN.md lines 942-948)
  - [x] Basic messages (text)
  - [x] Streaming text
  - [x] Tool use (function calling)
  - [x] PDFs/files (text note fallback; full support requires Responses API)
  - [x] Token counting endpoint (returns unsupported error)
  - [x] Batches endpoint (returns unsupported error)
- [x] Research: final API alignment audit -> `docs/final-audit.md`
- [x] Test: E2E flows (text, tools, streaming, files, errors), optional live API test (golden fixtures only; live test requires OPENAI_API_KEY)

## Phase 12: Release Infrastructure

- [x] MIT LICENSE file
- [x] README.md (quick start, configuration, endpoints, features, architecture, limitations)
- [x] Cargo.toml workspace metadata (description, license, repository)
- [x] GitHub Actions CI (fmt, clippy, build, test)
- [x] Production Dockerfile (multi-stage: rust builder, debian slim runtime)
- [x] Fix CLAUDE.md: incorrect claim that GET /metrics route didn't exist
- [x] CHANGELOG.md (version history for v0.1.0)

## Phase 13: Bug Fixes Found in Audit

- [x] Streaming retry: `chat_completion_stream()` now retries on 429/5xx with exponential backoff (was single-attempt)
- [x] Streaming metrics: spawned task now calls `record_success()`/`record_error()` (was request count only)
- [x] Concurrency limit scope: moved to API routes only; health/metrics bypass the limit
- [x] Dead code cleanup: renamed unused ToolCallAccumulator fields to `_id`/`_name`
- [x] Format fix: `middleware.rs` chain expression collapsed to single line per rustfmt

## Phase 14: Documentation and Validation

- [x] Add API documentation links to all public types and functions (87 items across 16 files)
- [x] Validate all types against official Anthropic and OpenAI API documentation
- [x] Add `BillingError` (HTTP 402) to `ErrorType` enum (was missing per Anthropic docs)
- [x] Competitive analysis against 3 similar proxy projects to identify gaps

## Phase 14b: Transparent Proxy Hardening
**Priority: high** | Target: Claude Code CLI drop-in via `ANTHROPIC_BASE_URL=http://localhost:3000`

The proxy silently drops fields and doesn't accept standard Anthropic headers. These changes make it work as a transparent proxy for Claude Code CLI.

- [x] Accept `anthropic-version` header in middleware (log value, don't reject if missing)
- [x] Accept `anthropic-beta` header in middleware (log value, don't reject if missing)
- [x] Add `tracing::warn` for lossy translations in `message_map.rs`:
  - [x] `metadata` present (dropped, no OpenAI equivalent)
  - [x] `stop_sequences` >4 items (truncated to OpenAI limit)
  - [x] `cache_control` on system blocks (dropped, no OpenAI equivalent)
  - [x] Document/PDF blocks (degraded to text note, not actual content)
  - [x] `top_k` present (dropped, no OpenAI equivalent)
- [x] Preserve `created` timestamp from OpenAI response in Anthropic response

## Phase 15: Configurable Model Mapping
**Priority: high** | Gap found in: [1rgs/claude-code-proxy](https://github.com/1rgs/claude-code-proxy), [SovranAMR/claude-code-via-antigravity](https://github.com/SovranAMR/claude-code-via-antigravity)

Currently model names pass through unchanged (e.g., `claude-sonnet-4-6` sent to OpenAI as-is, which fails). Other proxies map Claude model patterns to OpenAI models.

- [x] Model mapping config via env vars (`BIG_MODEL`, `SMALL_MODEL`)
- [x] Pattern matching: model names containing "haiku" -> SMALL_MODEL, "sonnet"/"opus" -> BIG_MODEL
- [x] Passthrough for unrecognized models (with warning log)
- [x] Update GET /v1/models to use dynamic handler with State
- [x] Test: model mapping, passthrough, case insensitive, custom values

## Phase 16: `max_completion_tokens` Migration
**Priority: medium** | Gap found in: [1rgs/claude-code-proxy](https://github.com/1rgs/claude-code-proxy)

OpenAI deprecated `max_tokens` in favor of `max_completion_tokens`. Our translation now uses the non-deprecated field.

- [x] Add `max_completion_tokens: Option<u32>` to `ChatCompletionRequest`
- [x] Map Anthropic `max_tokens` -> OpenAI `max_completion_tokens` in `anthropic_to_openai_request`
- [x] Keep `max_tokens` field on struct as fallback for older OpenAI-compatible backends
- [x] Test: field mapping verified (max_tokens is None, max_completion_tokens has value)

## Phase 17: Extended Thinking Support
**Priority: medium** | Gap found in: [SovranAMR/claude-code-via-antigravity](https://github.com/SovranAMR/claude-code-via-antigravity), [1rgs/claude-code-proxy](https://github.com/1rgs/claude-code-proxy)

`thinking` is a documented Anthropic feature (ThinkingConfigParam: enabled/disabled). Streaming includes `thinking_delta` events. OpenAI has no equivalent, so strip when targeting OpenAI.

- [x] Add `ThinkingConfig` types to `anthropic/messages.rs` (enabled with budget_tokens, disabled)
- [x] Add `Thinking` variant to `ContentBlock` enum (with thinking text and optional signature)
- [x] Add `thinking: Option<ThinkingConfig>` to `MessageCreateRequest`
- [x] Strip thinking config in `anthropic_to_openai_request` (OpenAI has no equivalent, warns)
- [x] Add `ThinkingDelta` variant to `Delta` enum in streaming types
- [x] Thinking blocks in assistant messages dropped silently in translation (via catch-all arm)
- [x] Test: thinking config serde roundtrip, stripping in translation, thinking delta serde, thinking block dropped

## Phase 18: `top_k` Parameter
**Priority: low** | Validated against: [Anthropic API docs](https://docs.anthropic.com/en/api/messages)

`top_k` is a documented Anthropic parameter. Now an explicit typed field instead of caught by serde flatten.

- [x] Add `top_k: Option<u32>` to `MessageCreateRequest`
- [x] Drop in `anthropic_to_openai_request` (no OpenAI equivalent, warns)
- [x] Test: field accepted as typed field, not in extra

## Phase 19: Token Counting Endpoint (not started)
**Priority: low** | Gap found in: [1rgs/claude-code-proxy](https://github.com/1rgs/claude-code-proxy), [jimmc414/claude_n_codex_api_proxy](https://github.com/jimmc414/claude_n_codex_api_proxy)

POST `/v1/messages/count_tokens` is a real Anthropic endpoint. Currently returns 400 "not supported". Other proxies implement via tokenizer libraries.

- [ ] Add `tiktoken-rs` dependency for OpenAI tokenizer
- [ ] Implement POST /v1/messages/count_tokens (replace stub)
- [ ] Convert Anthropic message format, count tokens via tiktoken
- [ ] Return `{"input_tokens": N}` response (approximate, model-dependent)
- [ ] Test: basic counting, empty messages, tool definitions

## Phase 20: Gemini Backend Research (complete)
**Priority: medium** | Reference: [SovranAMR/claude-code-via-antigravity](https://github.com/SovranAMR/claude-code-via-antigravity), [1rgs/claude-code-proxy](https://github.com/1rgs/claude-code-proxy)

Research phase only. Two competing proxies support Gemini as a backend. Need to determine how Gemini's API compares to OpenAI's Chat Completions API and what translation work would be required.

**Questions answered:**
- [x] Does Gemini offer an OpenAI-compatible endpoint? **Yes.** Vertex AI exposes `projects.locations.endpoints.openapi` with `completions`, `embeddings`, and `responses` resources. Existing OpenAI translation may work with different auth + base URL + model names.
- [x] What is Gemini's native API format? **`generateContent` / `streamGenerateContent`** using `contents[]/parts[]` model (not messages). `Part` is a union of text, inlineData, fileData, functionCall, functionResponse.
- [x] How does Gemini handle tool calling? **Different.** Uses `functionCall`/`functionResponse` as `Part` types within `contents[]`. Max 128 function declarations per request. Streaming supports `partialArgs[]` and `willContinue`. Schema must be OpenAPI 3.0 subset (restricted JSON Schema).
- [x] How does Gemini handle streaming? **SSE for Developer API** (`streamGenerateContent?alt=sse`). Vertex AI uses a stream of `GenerateContentResponse` instances (not explicitly SSE). Each event is a full response with incremental content.
- [x] How does Gemini handle system prompts? **Separate field.** `systemInstruction` is a `Content` object outside `contents[]`. Only text parts allowed. No `system` or `developer` role; `Content.role` is restricted to `user` or `model`.
- [x] How does Gemini handle images/vision? **`parts[]` with `inlineData` (base64 + MIME) or `fileData` (URI + MIME).** Vertex `fileData.fileUri` expects GCS URIs. Developer API uses Files API with Google resumable upload headers.
- [x] Authentication: **Three modes.** Developer API: API key via `x-goog-api-key` header or `?key=` query param. Vertex AI: OAuth bearer token (`Authorization: Bearer $(gcloud auth print-access-token)`). Live WebSocket: API key in URL query.
- [x] Role names and turn structure: **Strict.** `Content.role` is `user` or `model` only. Defaults to `user` if omitted. Gemini may enforce strict user/model alternation (consecutive same-role turns need merging).
- [x] What JSON Schema restrictions does Gemini impose on tool definitions? **Significant.** Rejects `$schema`, `$ref`, `$defs`, `definitions`, `default`, `pattern`, `examples`. Must strip `anyOf`/`oneOf` (rewrite to looser types). Schema is "subset of OpenAPI 3.0 schema object."
- [x] Token counting: **Yes.** `countTokens` endpoint exists for both Developer API and Vertex AI.
- [x] Model naming: **Different namespace.** Gemini models: `gemini-2.5-pro`, `gemini-2.5-flash`, etc. Need mapping from Claude model names (haiku -> flash, sonnet/opus -> pro).

**Deliverable:** `docs/gemini-api-diffs.md` documenting findings.

**Recommendation:** Implement Vertex OpenAI-compatible mode first (Phase 20a, minimal work). Build native Gemini translation (Phases 20c-20g) only if Vertex OpenAI-compatible mode has gaps (tool calling, streaming, schema handling).

## Phase 20a: Vertex AI OpenAI-Compatible Backend
**Priority: high** | Depends on P15 (model mapping), P20 (research)

Quick win: Vertex AI exposes an OpenAI-compatible Chat Completions endpoint. The existing Anthropic-to-OpenAI translation layer works as-is. Only need different auth, base URL, and model mapping.

- [x] Backend selection: `BACKEND` env var (`openai` default, `vertex` option)
- [x] Vertex config: `VERTEX_PROJECT`, `VERTEX_REGION` env vars
- [x] Vertex auth: `VERTEX_API_KEY` (via `x-goog-api-key` header) or `GOOGLE_ACCESS_TOKEN` (OAuth bearer)
- [x] URL construction: `https://{REGION}-aiplatform.googleapis.com/v1/projects/{PROJECT}/locations/{REGION}/endpoints/openapi/chat/completions`
- [x] Model name mapping: Claude names -> Gemini names (via Phase 15 model mapping infrastructure, defaults to gemini-2.5-pro/gemini-2.5-flash)
- [x] SSRF validation: `*.googleapis.com` hostnames pass existing validation (no changes needed)
- [x] Update `Config` struct with `BackendKind`, `BackendAuth`, keep existing OpenAI fields
- [x] Reuse `OpenAIClient` with different base URL and auth header (no new client needed)
- [ ] Research: validate Vertex OpenAI-compat endpoint supports streaming, tool calling, all Gemini models
- [x] Test: config parsing, URL construction, auth header selection, model mapping, SSRF validation
- [ ] Validate: manual test against real Vertex endpoint (non-streaming, streaming, tool calling)
- [ ] Document gaps found in Vertex OpenAI-compatible mode -> update `docs/gemini-api-diffs.md`

## Phase 20b: Backend Abstraction (not started)
**Priority: medium** | Depends on P20a

Currently `routes.rs` calls `OpenAIClient` directly. Before adding a native Gemini client, extract backend dispatch. Pure refactor, no behavior change.

- [ ] `BackendClient` enum: `OpenAI(OpenAIClient)` | `Vertex(OpenAIClient)` | future `Gemini(GeminiClient)`
- [ ] Move `chat_completion` and `chat_completion_stream` behind enum dispatch (not trait objects, matches codebase style)
- [ ] `AppState` holds `BackendClient` instead of `OpenAIClient`
- [ ] `routes.rs` calls backend-agnostic methods
- [ ] Test: all existing tests pass unchanged

## Phase 20c: Native Gemini Types (not started)
**Priority: low** | Depends on P20 | Only needed if Vertex OpenAI-compatible mode has gaps

Add Gemini-native request/response types to the translator crate. Pure types, no mapping logic.

- [ ] `crates/translator/src/gemini/` module with `mod.rs`
- [ ] `generate_content.rs`: `GenerateContentRequest`, `GenerateContentResponse`, `Candidate`, `Content`, `Part` (text, functionCall, functionResponse, inlineData, fileData)
- [ ] `streaming.rs`: streaming response shape (each chunk is a `GenerateContentResponse`)
- [ ] `errors.rs`: Gemini error response shape (`status`, `code`, `message`)
- [ ] `tools.rs`: `FunctionDeclaration`, `Tool`, `ToolConfig`, `FunctionCallingConfig`
- [ ] Research: validate types against current Gemini API docs (not just Phase 20 research)
- [ ] Test: serde round-trip from real Gemini API response fixtures
- [ ] Add `fixtures/gemini/` directory with golden files

## Phase 20d: Gemini Schema Sanitizer (not started)
**Priority: low** | Depends on P20c

Gemini's `FunctionDeclaration.parameters` accepts only a subset of JSON Schema. Schemas from Anthropic tool definitions need sanitization before forwarding to Gemini.

- [ ] `crates/translator/src/mapping/gemini_schema_map.rs`: `clean_gemini_schema(schema: &serde_json::Value) -> serde_json::Value`
- [ ] Strip unsupported keys: `$schema`, `$ref`, `$defs`, `definitions`, `default`, `pattern`, `examples`
- [ ] Rewrite `anyOf`/`oneOf` to nearest valid representation (first variant, or `string` fallback)
- [ ] Remove unsupported `format` values (keep only Gemini-supported ones)
- [ ] Enforce max 128 function declarations per request (truncate with warning)
- [ ] `tracing::warn` on every lossy transformation
- [ ] Test: each transformation, passthrough for already-valid schemas, edge cases (nested $ref, empty oneOf)

## Phase 20e: Gemini Message Mapping (not started)
**Priority: low** | Depends on P20c, P20d

Stateless mapping functions: Anthropic messages to Gemini `contents[]` and back.

- [ ] `crates/translator/src/mapping/gemini_message_map.rs`
- [ ] `anthropic_to_gemini_request(&MessageCreateRequest) -> GenerateContentRequest`
  - [ ] System prompt -> `system_instruction` field (text-only `Content`)
  - [ ] Role coercion: Anthropic `assistant` -> Gemini `model`, `user` -> `user`
  - [ ] Strict alternation: merge consecutive same-role turns
  - [ ] Content blocks: text -> `Part::Text`, image -> `Part::InlineData`, tool_use -> `Part::FunctionCall`, tool_result -> `Part::FunctionResponse`
  - [ ] Generation params: `temperature`, `max_tokens` -> `maxOutputTokens`, `top_p` -> `topP`, `stop_sequences` -> `stopSequences`
  - [ ] Tool definitions: use `clean_gemini_schema()` from P20d
- [ ] `gemini_to_anthropic_response(&GenerateContentResponse, model) -> MessageResponse`
  - [ ] `Candidate.content.parts[]` -> Anthropic content blocks
  - [ ] Finish reasons: `STOP` -> `end_turn`, `MAX_TOKENS` -> `max_tokens`, `SAFETY` -> `end_turn` (with warning)
  - [ ] Usage: `usageMetadata.promptTokenCount`/`candidatesTokenCount` -> Anthropic `input_tokens`/`output_tokens`
- [ ] Test: text, system prompt, tool use round-trip, multi-turn, role alternation merging, lossy transformations

## Phase 20f: Gemini Streaming Translation (not started)
**Priority: low** | Depends on P20e

Streaming state machine for native Gemini backend. Similar pattern to existing `StreamingTranslator` for OpenAI.

- [ ] `crates/translator/src/mapping/gemini_streaming_map.rs`
- [ ] `GeminiStreamingTranslator` state machine
- [ ] Gemini Dev API: SSE via `streamGenerateContent?alt=sse`, each event is a full `GenerateContentResponse` with incremental text
- [ ] Vertex AI streaming: stream of `GenerateContentResponse` instances (different framing, same payload)
- [ ] Map each chunk to Anthropic SSE events (message_start, content_block_start, content_block_delta, message_stop)
- [ ] Handle tool call streaming (functionCall parts, partialArgs)
- [ ] Test: text streaming, tool streaming, finish reasons, golden fixtures from `fixtures/gemini/`

## Phase 20g: Native Gemini Backend Client (not started)
**Priority: low** | Depends on P20b (backend abstraction), P20f (streaming)

Wire native Gemini translation into the proxy. Separate from Vertex OpenAI-compatible mode (P20a).

- [ ] `crates/proxy/src/backend/gemini_client.rs`: reqwest client calling `generateContent` / `streamGenerateContent`
- [ ] Auth: API key via query param (`?key=`) for Developer API, bearer token for Vertex native
- [ ] URL construction: `https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent` (Dev API)
- [ ] Add `Gemini(GeminiClient)` variant to `BackendClient` enum from P20b
- [ ] `BACKEND=gemini` config option (distinct from `vertex` which uses OpenAI-compatible endpoint)
- [ ] Retry logic: reuse existing retry/backoff strategy (Gemini uses similar 429/5xx semantics)
- [ ] Streaming: parse SSE from Gemini Dev API, feed through `GeminiStreamingTranslator`
- [ ] Update `routes.rs` dispatch to handle `Gemini` backend variant
- [ ] Test: non-streaming, streaming, error handling, tool calling (golden fixtures)
- [ ] Validate: manual test against Gemini Developer API

## Phase 21: Library Mode for Existing Codebases (not started)
**Priority: medium** | Enables using the translation layer as a library, not just a standalone proxy

The `anthropic_openai_translate` crate is already pure (no IO, no async, no network), but it's not designed for external consumption. This phase makes it a proper library that existing Rust, Python, and other codebases can embed directly, without running a separate proxy process.

**21a: Rust library ergonomics**
- [ ] Add top-level re-exports in `lib.rs` for common types (e.g., `pub use mapping::message_map::anthropic_to_openai_request`)
- [ ] Add convenience `translate_request()` and `translate_response()` functions that wrap the mapping layer
- [ ] Add a `TranslationConfig` struct (model mapping table, lossy behavior: warn/error/silent, feature flags)
- [ ] Builder pattern for `TranslationConfig` with sensible defaults
- [ ] Add `translate_stream_chunk()` convenience wrapper around `StreamingTranslator`
- [ ] Public error type for translation failures (currently mapping is infallible, but model mapping and validation may fail)
- [ ] Ensure all public types implement `Clone`, `Debug`, `Serialize`, `Deserialize`
- [ ] Add crate-level doc comment with usage examples (`//! # Examples`)
- [ ] Publish-ready `Cargo.toml` (categories, keywords, readme, documentation link)
- [ ] Test: library usage without proxy crate (standalone translator tests)

**21b: FFI / multi-language support (research)**
- [ ] Evaluate C FFI via `cbindgen` for Python/Node/Go bindings
- [ ] Evaluate WASM target (`wasm32-unknown-unknown`) for browser/edge use
- [ ] Evaluate PyO3 for native Python module (`pip install anthropic-openai-translate`)
- [ ] Document recommended integration pattern: embed library vs run proxy sidecar vs HTTP middleware
- [ ] Deliverable: `docs/library-integration.md` with examples for each approach

**21c: HTTP middleware mode**
- [ ] Extract an axum middleware/layer that can be dropped into existing axum/tower services
- [ ] The middleware intercepts Anthropic-format requests, translates, forwards to a configurable backend URL, translates response back
- [ ] Configurable via `TranslationConfig` from 21a
- [ ] Example: existing OpenAI-based axum service gains Anthropic API compatibility by adding one middleware layer
- [ ] Test: middleware integration test with mock backend

## Phase 22: Future Work (not started)

- [ ] OpenAI Responses API backend: wire up `ResponsesRequest`/`ResponsesResponse` types with runtime backend selection
- [ ] Live API integration tests (requires OPENAI_API_KEY, currently golden fixtures only)
- [ ] Error/edge case fixtures (4xx/5xx responses, oversized requests, malformed JSON)
- [ ] Graceful shutdown with in-flight request draining
- [ ] Rate limit header passthrough (OpenAI rate limit headers to Anthropic equivalents)
- [ ] Request/response logging toggle (opt-in body logging for debugging, redacted by default)
- [ ] Publish `anthropic_openai_translate` crate to crates.io
