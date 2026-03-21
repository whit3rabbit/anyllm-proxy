# Tasks: Anthropic-to-OpenAI Translation Proxy

Reference: [PLAN.md](./PLAN.md)

## Dependency Graph

```
P1 ──┬── P2 ──┬── P4 ──┬── P5 ──┬── P7 ──┐
     └── P3 ──┘        └── P6 ──┤        ├── P10 ── P11
                                ├── P8 ──┤
                                └── P9 ──┘
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
