# Tasks: LiteLLM Gap Fill + Rust Client Library

**Input**: Design documents from `/specs/20260325-120000-litellm-gap-fill/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/

**Organization**: Tasks grouped by user story (one per requirement). Each story is independently implementable and testable after the foundational phase.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1-US7)
- Exact file paths included in every task description

## Path Conventions

- **Translator crate**: `crates/translator/src/`
- **Proxy crate**: `crates/proxy/src/`
- **Client crate**: `crates/client/src/`
- **Integration tests**: `crates/proxy/tests/`

---

## Phase 1: Setup

**Purpose**: Add new dependencies and create empty module scaffolding

- [x] T001 Add `dashmap = "6"` to `crates/proxy/Cargo.toml` dependencies
- [x] T002 [P] Add `aws-sigv4 = { version = "1.4", features = ["sign-http"] }` and `aws-credential-types = "1.2"` to `crates/proxy/Cargo.toml` dependencies
- [x] T003 [P] Add feature-gated OTEL dependencies to `crates/proxy/Cargo.toml`: `opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp` (with `trace`, `http-proto`, `reqwest-client` features), `tracing-opentelemetry` all as optional under `[features] otel = [...]`
- [x] T004 Add `pub mod reverse_streaming_map;` to `crates/translator/src/mapping/mod.rs`

---

## Phase 2: Foundational (Reverse Translation Mapping)

**Purpose**: Pure translation functions required by US1. These are IO-free functions in the translator crate that convert OpenAI request types to Anthropic types and vice versa. MUST complete before US1 can begin.

- [x] T005 Implement `openai_to_anthropic_request(req: &ChatCompletionRequest) -> Result<MessageCreateRequest, TranslateError>` in `crates/translator/src/mapping/message_map.rs`. Must handle: system message extraction, user/assistant/tool message conversion, `tool_calls` -> `tool_use` blocks, `max_tokens`/`max_completion_tokens` -> `max_tokens` (reject if absent), `stop` -> `stop_sequences`, `temperature`/`top_p` passthrough. Drop unsupported fields (`presence_penalty`, `frequency_penalty`, `response_format`, `logprobs`, `n`, `seed`) and record them via `TranslationWarnings`.
- [x] T006 [P] Implement reverse stop_reason mapping helper in `crates/translator/src/mapping/message_map.rs`: `anthropic_stop_reason_to_openai(stop_reason: &StopReason) -> &str` mapping `end_turn`->`stop`, `max_tokens`->`length`, `tool_use`->`tool_calls`, `stop_sequence`->`stop`.
- [x] T007 Implement `anthropic_to_openai_response(resp: &MessageResponse, model: &str) -> ChatCompletionResponse` in `crates/translator/src/mapping/message_map.rs`. Must handle: text content concatenation, `tool_use` -> `tool_calls` (input object -> arguments string), thinking blocks -> `reasoning_content`, stop_reason mapping, usage mapping (reuse existing `anthropic_to_openai_usage`), generate `chatcmpl-` prefixed ID.
- [x] T008 Create `ReverseStreamingTranslator` struct in `crates/translator/src/mapping/reverse_streaming_map.rs`. Fields: `message_id`, `model`, `tool_call_index`, `input_tokens`, `output_tokens`. Implement `fn new(id: String, model: String) -> Self`.
- [x] T009 Implement `fn process_event(&mut self, event: &StreamEvent) -> Vec<ChatCompletionChunk>` on `ReverseStreamingTranslator` in `crates/translator/src/mapping/reverse_streaming_map.rs`. Map: `message_start` -> first chunk with `role: "assistant"`, `content_block_delta(TextDelta)` -> `delta.content`, `content_block_start(ToolUse)` -> `delta.tool_calls[index]` with id/name, `content_block_delta(InputJsonDelta)` -> `delta.tool_calls[index].function.arguments`, `content_block_delta(ThinkingDelta)` -> `delta.reasoning_content`, `message_delta` -> `finish_reason` chunk, `message_stop` -> `[DONE]` sentinel.
- [x] T010 Add unit tests for `openai_to_anthropic_request` in `crates/translator/src/mapping/message_map.rs` `#[cfg(test)]` module: basic message conversion, system message extraction, tool call conversion, missing max_tokens rejection, lossy field warnings.
- [x] T011 [P] Add unit tests for `anthropic_to_openai_response` in `crates/translator/src/mapping/message_map.rs` `#[cfg(test)]` module: text response, tool use response, thinking blocks, stop reason mapping, usage mapping.
- [x] T012 [P] Add unit tests for `ReverseStreamingTranslator` in `crates/translator/src/mapping/reverse_streaming_map.rs` `#[cfg(test)]` module: text streaming, tool call streaming with index tracking, thinking content, finish reason, `[DONE]` emission.
- [x] T013 Add convenience wrappers `translate_openai_to_anthropic_request` and `translate_anthropic_to_openai_response` in `crates/translator/src/translate.rs`. Re-export `ReverseStreamingTranslator` from `crates/translator/src/lib.rs`.

**Checkpoint**: `cargo test -p anyllm_translate` passes with all new reverse translation tests. All mapping functions are pure (no IO).

---

## Phase 3: User Story 1 - OpenAI Chat Completions Input (Priority: P1, MVP)

**Goal**: Accept `POST /v1/chat/completions` in OpenAI format, translate through Anthropic pipeline, return OpenAI format. Highest-value feature: unblocks all OpenAI-native clients.

**Independent Test**: `curl -X POST http://localhost:3000/v1/chat/completions -H "x-api-key: test" -H "Content-Type: application/json" -d '{"model":"claude-sonnet-4-20250514","messages":[{"role":"user","content":"Hello"}],"max_tokens":100}'` returns OpenAI-format JSON.

### Implementation for User Story 1

- [x] T014 [US1] Create `crates/proxy/src/server/chat_completions.rs` with non-streaming handler: extract `Json<ChatCompletionRequest>`, call `openai_to_anthropic_request`, dispatch to `BackendClient`, call `anthropic_to_openai_response`, return `Json<ChatCompletionResponse>`. Set `x-anyllm-degradation` header from `TranslationWarnings`. Return OpenAI-shaped errors on validation failure (missing max_tokens -> 400 `invalid_request_error`).
- [x] T015 [US1] Add streaming handler in `crates/proxy/src/server/chat_completions.rs`: when `stream: true`, dispatch to backend streaming path, create `ReverseStreamingTranslator`, emit `text/event-stream` with `data: {chunk}\n\n` lines (no `event:` prefix, matching OpenAI SSE format). Terminate with `data: [DONE]\n\n`.
- [x] T016 [US1] Register `POST /v1/chat/completions` route in `crates/proxy/src/server/routes.rs` on the existing backend router. Apply same middleware (auth, request ID, size limit, concurrency limit) as the `/v1/messages` route.
- [x] T017 [US1] Add integration tests in `crates/proxy/tests/` (new file `chat_completions.rs` or extend existing): non-streaming basic response, streaming basic response, tool call round-trip, missing max_tokens returns 400, degradation header set for lossy fields, empty messages returns 400.

**Checkpoint**: `cargo test -p anyllm_proxy` passes. `POST /v1/chat/completions` works end-to-end with a mock or live backend.

---

## Phase 4: User Story 2 - Azure OpenAI Backend (Priority: P1)

**Goal**: `BACKEND=azure` routes requests through Azure OpenAI using deployment-scoped URLs and `api-key` header. Reuses existing OpenAI client code.

**Independent Test**: `BACKEND=azure AZURE_OPENAI_ENDPOINT=https://... AZURE_OPENAI_DEPLOYMENT=gpt4o AZURE_OPENAI_API_KEY=... cargo run -p anyllm_proxy` starts and responds to `/v1/messages`.

### Implementation for User Story 2

- [x] T018 [US2] Add `BackendKind::AzureOpenAI` variant to the backend enum in `crates/proxy/src/config/mod.rs`. Parse `AZURE_OPENAI_API_KEY`, `AZURE_OPENAI_ENDPOINT`, `AZURE_OPENAI_DEPLOYMENT`, `AZURE_OPENAI_API_VERSION` (default `"2024-10-21"`) from env. Construct full URL: `{endpoint}/openai/deployments/{deployment}/chat/completions?api-version={version}` at config load time. Validate URL with existing `validate_url` function.
- [x] T019 [US2] Add `BackendAuth::AzureApiKey(String)` variant (or equivalent) in `crates/proxy/src/backend/mod.rs`. Map it to `RequestAuth::Header { name: "api-key", value: key }` in the auth application logic. Add `BackendClient::AzureOpenAI(OpenAIClient)` variant that constructs `OpenAIClient` with the pre-built Azure URL and `AzureApiKey` auth.
- [x] T020 [US2] Modify `crates/proxy/src/backend/openai_client.rs` to accept Azure's pre-constructed URL. The `chat_completions_url` for Azure is the full URL from config (no `/v1/chat/completions` suffix appended). Ensure the `model` field in the request body is still populated (Azure ignores it but accepts it).
- [x] T021 [US2] Add `#[ignore]` integration test in `crates/proxy/tests/` for Azure backend: send a request via the proxy configured with `BACKEND=azure`, verify response is valid Anthropic format. Requires `AZURE_OPENAI_API_KEY` env var to run.
- [x] T022 [US2] Update `docs/ENV.md` with Azure-specific env vars and usage example.

**Checkpoint**: `cargo build` clean. Azure config parsing tested. `#[ignore]` live test exists.

---

## Phase 5: User Story 3 - AWS Bedrock Backend (Priority: P1)

**Goal**: `BACKEND=bedrock` routes requests through AWS Bedrock using SigV4-signed requests. Non-streaming via `InvokeModel`, streaming via `InvokeModelWithResponseStream` with binary event stream decoding.

**Independent Test**: `BACKEND=bedrock AWS_REGION=us-east-1 AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... BIG_MODEL=anthropic.claude-3-5-sonnet-20241022-v2:0 cargo run -p anyllm_proxy` starts and responds.

### Implementation for User Story 3

- [ ] T023 [US3] Create `crates/proxy/src/backend/bedrock_client.rs` with `BedrockClient` struct. Fields: `http_client: reqwest::Client`, `region: String`, `credentials: aws_credential_types::Credentials`, `big_model: String`, `small_model: String`. Implement `fn new(config: &BedrockConfig, http_client: reqwest::Client) -> Self`.
- [ ] T024 [US3] Implement non-streaming `send_request` on `BedrockClient` in `crates/proxy/src/backend/bedrock_client.rs`: build Bedrock URL (`https://bedrock-runtime.{region}.amazonaws.com/model/{model_id}/invoke`), serialize Anthropic request body with `anthropic_version: "bedrock-2023-05-31"` (model field omitted from body), sign request with `aws_sigv4::http_request::sign()`, send via reqwest, deserialize response as `MessageResponse`.
- [ ] T025 [US3] Implement AWS Event Stream binary frame decoder in `crates/proxy/src/backend/bedrock_client.rs` (or a submodule): parse 4-byte prelude length, 4-byte headers length, headers, payload, 4-byte CRC32 checksum. Extract `chunk.bytes` field, base64-decode to get Anthropic SSE JSON. Target: ~80 lines.
- [ ] T026 [US3] Implement streaming `send_request_stream` on `BedrockClient` in `crates/proxy/src/backend/bedrock_client.rs`: build URL with `/invoke-with-response-stream`, sign request, send via reqwest with streaming response, pipe response bytes through event stream decoder, yield Anthropic `StreamEvent` items compatible with existing `StreamingTranslator`.
- [ ] T027 [US3] Add `BackendKind::Bedrock` to `crates/proxy/src/config/mod.rs`. Parse `AWS_REGION`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN` (optional) from env. Store as `BedrockConfig`. Add `BackendClient::Bedrock(BedrockClient)` variant to `crates/proxy/src/backend/mod.rs` and wire through dispatch.
- [ ] T028 [US3] Add unit tests for event stream decoder in `crates/proxy/src/backend/bedrock_client.rs` `#[cfg(test)]` module: parse a known binary frame, extract payload, verify CRC, handle partial frames.
- [ ] T029 [US3] Add `#[ignore]` integration test in `crates/proxy/tests/` for Bedrock backend: non-streaming and streaming paths. Requires AWS credentials.
- [ ] T030 [US3] Update `docs/ENV.md` with Bedrock-specific env vars and usage example.

**Checkpoint**: `cargo build` clean. Event stream decoder unit tests pass. `#[ignore]` live tests exist.

---

## Phase 6: User Story 4 - Virtual Key Management (Priority: P1)

**Goal**: Admin API for creating, listing, and revoking API keys stored in SQLite. Keys take effect immediately without proxy restart.

**Independent Test**: `POST /admin/api/keys` returns a key; that key authenticates against `/v1/messages`; `DELETE /admin/api/keys/{id}` revokes it; subsequent requests with that key return 401.

### Implementation for User Story 4

- [x] T031 [US4] Add `virtual_api_key` table DDL to `crates/proxy/src/admin/db.rs` in the existing `init_db` function (or equivalent). Schema per `data-model.md`: `id INTEGER PRIMARY KEY AUTOINCREMENT`, `key_hash TEXT NOT NULL UNIQUE`, `key_prefix TEXT NOT NULL`, `description TEXT`, `created_at TEXT NOT NULL`, `expires_at TEXT`, `revoked_at TEXT`, `spend_limit REAL`, `rpm_limit INTEGER`, `tpm_limit INTEGER`, `total_spend REAL NOT NULL DEFAULT 0`, `total_requests INTEGER NOT NULL DEFAULT 0`, `total_tokens INTEGER NOT NULL DEFAULT 0`. Add index on `key_hash`.
- [x] T032 [US4] Create `crates/proxy/src/admin/keys.rs` with key generation and hashing. `fn generate_virtual_key() -> (String, String, [u8; 32])` returns `(raw_key, key_prefix, key_hash)`. Use two UUID v4s concatenated with `sk-vk` prefix for the raw key. Hash with SHA-256 (reuse existing `sha2` dependency). `key_prefix` is first 8 chars.
- [x] T033 [US4] Add CRUD functions in `crates/proxy/src/admin/db.rs`: `insert_virtual_key(conn, key_hash, key_prefix, description, expires_at, rpm_limit, tpm_limit, spend_limit)`, `list_virtual_keys(conn) -> Vec<VirtualKeyRow>`, `revoke_virtual_key(conn, id) -> Option<VirtualKeyRow>`, `load_active_virtual_keys(conn) -> Vec<VirtualKeyRow>`.
- [x] T034 [US4] Add `DashMap<[u8; 32], VirtualKeyMeta>` to `SharedState` in `crates/proxy/src/admin/state.rs` (or wherever `SharedState` is defined). On startup in `crates/proxy/src/main.rs`, call `load_active_virtual_keys` and populate the DashMap.
- [x] T035 [US4] Add admin API endpoints in `crates/proxy/src/admin/routes.rs`: `POST /admin/api/keys` (create key, insert to DB, insert to DashMap, return raw key once), `GET /admin/api/keys` (list from DB with computed status), `DELETE /admin/api/keys/{id}` (set `revoked_at` in DB, remove from DashMap, return confirmation).
- [x] T036 [US4] Extend auth middleware in `crates/proxy/src/server/middleware.rs` to check the DashMap after checking env-var keys. SHA-256 hash the incoming credential, look up in DashMap, verify `revoked_at` is None and `expires_at` is not past. If both checks fail, return 401.
- [x] T037 [US4] Add unit tests for key generation and hashing in `crates/proxy/src/admin/keys.rs` `#[cfg(test)]` module: key format, prefix extraction, hash determinism.
- [ ] T038 [US4] Add integration tests for virtual key admin API in `crates/proxy/tests/`: create key, list keys, use key for auth, revoke key, verify revoked key is rejected.

**Checkpoint**: `cargo test -p anyllm_proxy` passes. Virtual key CRUD works. Key revocation is immediate.

---

## Phase 7: User Story 5 - Rust Client Library Improvements (Priority: P1)

**Goal**: `anyllm_client` becomes a first-class Rust SDK with builder pattern, typed streaming, and tool helpers.

**Independent Test**: `cargo doc -p anyllm_client --no-deps` builds without warnings. `cargo test -p anyllm_client` passes.

### Implementation for User Story 5

- [x] T039 [P] [US5] Add `ClientBuilder` to `crates/client/src/client.rs` with method chaining: `fn new() -> Self`, `fn base_url(mut self, url: &str) -> Self`, `fn api_key(mut self, key: &str) -> Self`, `fn timeout(mut self, d: Duration) -> Self`, `fn read_timeout(mut self, d: Duration) -> Self`, `fn max_retries(mut self, n: u32) -> Self`, `fn tls_config(mut self, cfg: TlsConfig) -> Self`, `fn build(self) -> Result<Client, ClientError>`. Implement `Client::builder() -> ClientBuilder` convenience method.
- [x] T040 [P] [US5] Create `crates/client/src/tools.rs` with `ToolBuilder` and `ToolChoiceBuilder`. `ToolBuilder`: `fn new(name: &str) -> Self`, `fn description(mut self, desc: &str) -> Self`, `fn input_schema(mut self, schema: Value) -> Self`, `fn build(self) -> Tool`. `ToolChoiceBuilder`: `fn auto() -> ToolChoice`, `fn any() -> ToolChoice`, `fn none() -> ToolChoice`, `fn specific(name: &str) -> ToolChoice`.
- [x] T041 [US5] Add streaming return type to `crates/client/src/client.rs`: `fn messages_stream(&self, req: MessageCreateRequest) -> Result<impl Stream<Item = Result<StreamEvent, ClientError>>, ClientError>`. Parse SSE frames from the reqwest response byte stream, deserialize each `data:` line into `StreamEvent`.
- [x] T042 [US5] Update `crates/client/src/lib.rs` to re-export all public types: `Client`, `ClientBuilder`, `ClientConfig`, `ClientError`, `Tool`, `ToolBuilder`, `ToolChoice`, `ToolChoiceBuilder`, `StreamEvent`, and all Anthropic request/response types from `anyllm_translate`.
- [x] T043 [US5] Add rustdoc examples to all public types and methods in `crates/client/src/client.rs`, `crates/client/src/tools.rs`, and `crates/client/src/lib.rs`. Each builder method and each public function gets a `/// # Examples` block.
- [x] T044 [US5] Bump `anyllm_client` version to `0.2.0` in `crates/client/Cargo.toml`.
- [x] T045 [US5] Add unit tests for `ClientBuilder` (valid build, missing required fields), `ToolBuilder`, and `ToolChoiceBuilder` in their respective `#[cfg(test)]` modules.

**Checkpoint**: `cargo doc -p anyllm_client --no-deps` builds clean. `cargo test -p anyllm_client` passes.

---

## Phase 8: User Story 6 - Per-Key Rate Limiting (Priority: P2, depends on US4)

**Goal**: RPM and TPM limits per virtual key with sliding window enforcement. Returns 429 with `retry-after` when exceeded.

**Independent Test**: Create a key with `rpm_limit: 2`, send 3 requests, third returns 429.

### Implementation for User Story 6

- [x] T046 [US6] Add `RateLimitState` struct to `crates/proxy/src/admin/keys.rs`: `rpm_window: Mutex<VecDeque<u64>>`, `tpm_window: Mutex<VecDeque<(u64, u32)>>`. Add `fn check_rpm(&self, limit: u32) -> Result<(), Duration>` (returns Ok or Err with retry-after duration) and `fn record_rpm(&self)`. Same pattern for TPM: `fn check_tpm(&self, limit: u32, tokens: u32) -> Result<(), Duration>` and `fn record_tpm(&self, tokens: u32)`. Drain entries older than 60 seconds on each check.
- [x] T047 [US6] Add `rate_state: Arc<RateLimitState>` field to `VirtualKeyMeta` in DashMap. Initialize a new `RateLimitState` for each key loaded on startup and each key created via admin API.
- [x] T048 [US6] Extend auth middleware in `crates/proxy/src/server/middleware.rs`: after virtual key validation passes, check `rate_state.check_rpm(key.rpm_limit)`. If exceeded, return HTTP 429 with `retry-after: {seconds}` header and OpenAI-shaped rate limit error body. TPM check happens after the response (post-middleware or in the handler) since token count is only known after the backend responds.
- [ ] T049 [US6] Add post-response TPM recording: after the backend response is received and token count is known, call `rate_state.record_tpm(output_tokens)`. If TPM would be exceeded, the next request's pre-check catches it.
- [x] T050 [US6] Add unit tests for `RateLimitState` in `crates/proxy/src/admin/keys.rs` `#[cfg(test)]` module: window expiry, RPM enforcement, TPM enforcement, concurrent access safety.
- [ ] T051 [US6] Add integration test for rate limiting in `crates/proxy/tests/`: create key with `rpm_limit: 2`, send 2 requests (200), send 3rd request (429 with `retry-after` header).

**Checkpoint**: `cargo test -p anyllm_proxy` passes. Rate limiting enforced per-key.

---

## Phase 9: User Story 7 - OpenTelemetry Export (Priority: P2)

**Goal**: Optional OTEL span export via feature flag. When enabled, all request spans are exported to an OTLP collector with request metadata as attributes.

**Independent Test**: `cargo build -p anyllm_proxy --features otel` compiles. With a local OTEL collector running, spans appear in the collector UI.

### Implementation for User Story 7

- [ ] T052 [US7] Create `crates/proxy/src/otel.rs` behind `#[cfg(feature = "otel")]`. Implement `fn init_otel() -> OtelGuard`: build `SdkTracerProvider` with `opentelemetry-otlp` `SpanExporter` (http-proto, reqwest-client), set global tracer provider, set `TraceContextPropagator`. Return `OtelGuard` struct whose `Drop` impl calls `provider.shutdown()`.
- [ ] T053 [US7] Modify tracing subscriber initialization in `crates/proxy/src/main.rs`: under `#[cfg(feature = "otel")]`, add `OpenTelemetryLayer::new(tracer)` to the existing `tracing_subscriber::registry()` chain. Store `OtelGuard` in a variable that lives for the duration of `main`. Ensure the non-otel path is unchanged via `#[cfg(not(feature = "otel"))]`.
- [ ] T054 [US7] Add span attributes to request handlers: in the existing request middleware or handler instrumentation, record `http.request.id`, `gen_ai.request.model`, `gen_ai.response.model`, `http.response.status_code`, `gen_ai.usage.input_tokens`, `gen_ai.usage.output_tokens` via `tracing::Span::current().record(...)`. Ensure the `#[tracing::instrument]` macros declare these fields.
- [ ] T055 [US7] Verify `cargo build -p anyllm_proxy` (without `otel` feature) still compiles and has no OTEL dependencies. Verify `cargo build -p anyllm_proxy --features otel` compiles clean.
- [ ] T056 [US7] Update `docs/ENV.md` with OTEL-related env vars: `OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_SERVICE_NAME`, `OTEL_TRACES_SAMPLER`. Document the `--features otel` build flag.

**Checkpoint**: Both `cargo build` (default) and `cargo build --features otel` compile. No runtime overhead when feature is off.

---

## Phase 10: Polish and Cross-Cutting Concerns

**Purpose**: Final validation, documentation updates, and CI adjustments

- [x] T057 [P] Update `docs/COMPARISON_LITELLM.md` to reflect closed gaps: `POST /v1/chat/completions` input, Bedrock backend, Azure backend, virtual key management, per-key rate limiting, OTEL export. Move items from "Major gap" to "Advantage" or "Parity" as appropriate.
- [ ] T058 [P] Update `CLAUDE.md` with new backend types, new env vars, new admin endpoints, new source files, and updated test counts.
- [ ] T059 [P] Update `README.md` with quickstart examples for new features (reference `quickstart.md` content).
- [ ] T060 Run `cargo clippy -- -D warnings` across all crates and fix any warnings.
- [ ] T061 Run `cargo fmt --check` and fix any formatting issues.
- [ ] T062 Run `cargo test` full suite and verify all tests pass (expect ~550+ tests).
- [ ] T063 Verify all new source files are under 400 lines (excluding `#[cfg(test)]` modules).

---

## Dependencies and Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies, start immediately
- **Phase 2 (Foundational)**: Depends on Phase 1 (T004 specifically)
- **Phase 3 (US1)**: Depends on Phase 2 completion
- **Phase 4 (US2)**: Depends on Phase 1 only (independent of Phase 2)
- **Phase 5 (US3)**: Depends on Phase 1 only (independent of Phase 2)
- **Phase 6 (US4)**: Depends on Phase 1 only (independent of Phase 2)
- **Phase 7 (US5)**: Depends on Phase 1 only (independent)
- **Phase 8 (US6)**: Depends on Phase 6 (US4) completion
- **Phase 9 (US7)**: Depends on Phase 1 (T003 specifically)
- **Phase 10 (Polish)**: Depends on all user stories

### User Story Dependencies

- **US1 (Chat Completions)**: Requires Phase 2 (reverse translation). Critical path.
- **US2 (Azure)**: Independent. Can start after Phase 1.
- **US3 (Bedrock)**: Independent. Can start after Phase 1.
- **US4 (Virtual Keys)**: Independent. Can start after Phase 1.
- **US5 (Client Library)**: Independent. Can start after Phase 1.
- **US6 (Rate Limiting)**: Depends on US4 completion.
- **US7 (OTEL)**: Independent. Can start after Phase 1.

### Within Each User Story

- Types/models before services
- Services before handlers/endpoints
- Core implementation before integration tests
- Unit tests alongside implementation

### Parallel Opportunities

After Phase 1 completes, up to 5 user stories can proceed in parallel:

```
Phase 1 (Setup)
    |
    +---> Phase 2 (Foundational) ---> Phase 3 (US1: Chat Completions)
    |
    +---> Phase 4 (US2: Azure)
    |
    +---> Phase 5 (US3: Bedrock)
    |
    +---> Phase 6 (US4: Virtual Keys) ---> Phase 8 (US6: Rate Limiting)
    |
    +---> Phase 7 (US5: Client Library)
    |
    +---> Phase 9 (US7: OTEL)
```

---

## Parallel Example: After Phase 1

```
# These can all run simultaneously:
Agent 1: Phase 2 (T005-T013) -> Phase 3 (T014-T017)
Agent 2: Phase 4 (T018-T022) Azure backend
Agent 3: Phase 5 (T023-T030) Bedrock backend
Agent 4: Phase 6 (T031-T038) Virtual keys -> Phase 8 (T046-T051) Rate limiting
Agent 5: Phase 7 (T039-T045) Client library
Agent 6: Phase 9 (T052-T056) OTEL
```

---

## Implementation Strategy

### MVP First (US1 Only)

1. Complete Phase 1: Setup (T001-T004)
2. Complete Phase 2: Foundational reverse translation (T005-T013)
3. Complete Phase 3: US1 Chat Completions endpoint (T014-T017)
4. **STOP and VALIDATE**: `POST /v1/chat/completions` works with curl
5. This alone closes the single largest adoption gap

### Incremental Delivery

1. Setup + Foundational + US1 -> Chat Completions works (MVP)
2. Add US2 (Azure) -> Enterprise Azure users unblocked
3. Add US3 (Bedrock) -> Enterprise AWS users unblocked
4. Add US4 (Virtual Keys) -> Dynamic key management
5. Add US5 (Client Library) -> First-class Rust SDK
6. Add US6 (Rate Limiting) -> Per-key enforcement
7. Add US7 (OTEL) -> Observability integration
8. Polish phase -> Documentation and CI

Each story adds value independently without breaking previous stories.

---

## Notes

- [P] tasks = different files, no dependencies on incomplete tasks
- [USn] label maps task to specific user story
- Translator crate must remain IO-free (no reqwest, no tokio, no file access)
- All new files must be under 400 lines (excluding `#[cfg(test)]` modules)
- Commit after each task or logical group
- `cargo clippy -- -D warnings` must stay clean throughout
