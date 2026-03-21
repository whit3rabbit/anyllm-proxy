# PRD: Phase 6 - Proxy Server and Routing

## Introduction

Wire the translation logic into a working HTTP proxy. Implement the axum routes, middleware (auth, request ID, size limits, logging), the reqwest-based OpenAI client, and the non-streaming `POST /v1/messages` endpoint. This is where the translator library meets the network.

## Goals

- Implement `POST /v1/messages` (non-streaming) as a full request/response proxy
- Add authentication middleware that validates `x-api-key` headers
- Add request ID generation and correlation
- Enforce 32MB request size limit
- Implement the OpenAI backend client using reqwest
- Handle upstream errors and translate them to Anthropic error shapes

## User Stories

### US-001: POST /v1/messages route (non-streaming)
**Description:** As a client using the Anthropic SDK, I need to POST to `/v1/messages` and get back an Anthropic-shaped response, even though the proxy is calling OpenAI behind the scenes.

**Acceptance Criteria:**
- [ ] `POST /v1/messages` accepts JSON body matching `AnthropicMessageCreateRequest`
- [ ] Request with `stream: false` or `stream` absent triggers non-streaming path
- [ ] Response is `AnthropicMessageCreateResponse` JSON with correct content-type
- [ ] Request is translated to OpenAI, sent to OpenAI, response translated back
- [ ] Returns 200 on success with valid Anthropic response shape

### US-002: Configuration
**Description:** As an operator, I need the proxy to read configuration from environment variables so I can deploy it without code changes.

**Acceptance Criteria:**
- [ ] `OPENAI_API_KEY`: required, used in `Authorization: Bearer` header to OpenAI
- [ ] `OPENAI_BASE_URL`: optional, defaults to `https://api.openai.com`
- [ ] `LISTEN_PORT`: optional, defaults to `3000`
- [ ] `RUST_LOG`: optional, controls tracing filter
- [ ] Missing `OPENAI_API_KEY` at startup logs a warning (still starts, fails on requests)

### US-003: Authentication middleware
**Description:** As a developer, I need the proxy to validate that incoming requests include the `x-api-key` header, rejecting unauthenticated requests with the correct Anthropic error shape.

**Acceptance Criteria:**
- [ ] Requests without `x-api-key` header -> 401 `authentication_error` response
- [ ] Requests with empty `x-api-key` -> 401 `authentication_error` response
- [ ] The `x-api-key` value is NOT forwarded to OpenAI (proxy uses its own configured key)
- [ ] `anthropic-version` header is accepted but not enforced (log if missing)
- [ ] Test: missing auth -> 401, present auth -> passes through

### US-004: Request ID middleware
**Description:** As an operator debugging issues, I need every request to have a unique ID that appears in logs and response headers.

**Acceptance Criteria:**
- [ ] Generate UUID v4 request ID for each incoming request
- [ ] If client sends `x-request-id` or `request-id` header, use that instead
- [ ] Include request ID in response header `request-id`
- [ ] Request ID available in tracing span for all log lines
- [ ] Test: response includes `request-id` header

### US-005: Request size limit
**Description:** As an operator, I need requests larger than 32MB rejected to match Anthropic's documented limit and prevent memory issues.

**Acceptance Criteria:**
- [ ] Requests with `Content-Length` > 32MB -> 413 `request_too_large` error
- [ ] Error response matches Anthropic error shape
- [ ] Test: oversized request gets 413

### US-006: OpenAI backend client
**Description:** As a developer, I need a reqwest client that sends translated requests to OpenAI and returns responses.

**Acceptance Criteria:**
- [ ] `OpenAIClient` struct wrapping `reqwest::Client`
- [ ] `send_chat_completion(&self, req: OpenAIChatCompletionRequest) -> Result<OpenAIChatCompletionResponse, ProxyError>`
- [ ] Sets `Authorization: Bearer {api_key}` header
- [ ] Sets `Content-Type: application/json`
- [ ] Configurable base URL (`{base_url}/v1/chat/completions`)
- [ ] Timeouts: connect 10s, read 120s
- [ ] Returns typed error for HTTP errors, connection failures, JSON parse failures

### US-007: Upstream error handling
**Description:** As a developer, I need upstream OpenAI errors translated to Anthropic error shapes so clients see consistent error responses.

**Acceptance Criteria:**
- [ ] OpenAI 4xx/5xx -> translated to Anthropic error using `errors_map` from Phase 4
- [ ] OpenAI error body parsed and message included in Anthropic error
- [ ] Connection timeout -> Anthropic `api_error` (500)
- [ ] JSON parse error -> Anthropic `api_error` (500) with descriptive message
- [ ] Test: mock upstream returning 429, verify proxy returns Anthropic-shaped 429

### US-008: Logging middleware
**Description:** As an operator, I need structured request/response logging for observability.

**Acceptance Criteria:**
- [ ] Log on request: method, path, request ID, content length
- [ ] Log on response: status code, request ID, latency
- [ ] Use `tracing` with structured fields (not string interpolation)
- [ ] Sensitive headers (`x-api-key`, `Authorization`) are NOT logged
- [ ] Controlled by `RUST_LOG` env var

## Functional Requirements

- FR-1: `POST /v1/messages` with `stream: false` performs full translate-forward-translate cycle
- FR-2: `GET /health` continues to return 200 (from Phase 1)
- FR-3: All error responses match Anthropic `{type: "error", error: {type, message}}` shape
- FR-4: Request size limit enforced at 32MB
- FR-5: Middleware ordering: size limit -> auth -> request ID -> logging -> route handler

## Non-Goals

- No streaming support (Phase 7)
- No model name aliasing/mapping
- No retry logic (Phase 10)
- No rate limit header translation (Phase 10)
- No `/v1/models` or other compatibility endpoints (Phase 9)

## Technical Considerations

- Use axum's `DefaultBodyLimit` for size enforcement
- Use `tower` layers for middleware composition
- reqwest client should be shared (connection pooling) via axum state
- Consider `axum::extract::State` for sharing config and client
- Use `tracing::instrument` for automatic span creation on route handlers

## Success Metrics

- Integration test: POST valid Anthropic request to proxy with mocked OpenAI backend, get valid Anthropic response
- Integration test: POST without auth -> 401
- Integration test: POST oversized body -> 413
- Integration test: Upstream error -> correct Anthropic error shape
- `cargo test` passes for both crates
