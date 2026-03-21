# PRD: Phase 9 - Compatibility Endpoints

## Introduction

Implement additional Anthropic API endpoints beyond `/v1/messages` to improve SDK compatibility: `GET /v1/models` for model listing, `POST /v1/messages/count_tokens` for token estimation, and `POST /v1/messages/batches` as an explicit unsupported endpoint. These endpoints round out the API surface so Anthropic SDKs work without unexpected 404s.

## Goals

- Implement `GET /v1/models` returning a model list (proxied from OpenAI or static mapping)
- Implement `POST /v1/messages/count_tokens` with local approximation or explicit unsupported error
- Implement `POST /v1/messages/batches` returning explicit unsupported error
- Return proper Anthropic error shapes for unknown routes

## User Stories

### US-001: Models list endpoint
**Description:** As a client using the Anthropic SDK, I need `GET /v1/models` to return a list of available models so model discovery works.

**Acceptance Criteria:**
- [ ] `GET /v1/models` returns 200 with JSON body matching Anthropic models list shape
- [ ] Option A (proxy): fetch OpenAI models and translate names, or
- [ ] Option B (static): return configured model mapping from config/env
- [ ] Response shape: `{data: [{id, display_name, type, ...}], ...}` matching Anthropic format
- [ ] Test: endpoint returns valid model list

### US-002: Count tokens endpoint
**Description:** As a client using the Anthropic SDK, I need `POST /v1/messages/count_tokens` to either return an approximation or a clear error so the client handles it gracefully.

**Acceptance Criteria:**
- [ ] `POST /v1/messages/count_tokens` accepts same body as `/v1/messages`
- [ ] Option A (approximate): return rough token count based on character heuristic
- [ ] Option B (unsupported): return 501 or Anthropic error with clear message
- [ ] Response shape matches Anthropic's count_tokens response if implemented
- [ ] Test: endpoint returns expected response or error

### US-003: Batches endpoint (unsupported)
**Description:** As a client, I need `POST /v1/messages/batches` to return a clear error rather than a generic 404.

**Acceptance Criteria:**
- [ ] `POST /v1/messages/batches` returns Anthropic error: `{type: "error", error: {type: "not_found_error", message: "Batch API is not supported by this proxy"}}`
- [ ] HTTP status: 404 or 501
- [ ] Test: endpoint returns expected error shape

### US-004: Unknown route handling
**Description:** As a client, I need unknown routes to return Anthropic-shaped 404 errors instead of axum defaults.

**Acceptance Criteria:**
- [ ] Any unmatched route returns `{type: "error", error: {type: "not_found_error", message: "Not found"}}` with 404 status
- [ ] Response has `Content-Type: application/json`
- [ ] Test: GET /v1/nonexistent returns Anthropic 404

## Functional Requirements

- FR-1: `GET /v1/models` returns a valid response (proxied or static)
- FR-2: `POST /v1/messages/count_tokens` returns approximation or explicit unsupported error
- FR-3: `POST /v1/messages/batches` returns explicit unsupported error
- FR-4: All error responses use Anthropic error shape
- FR-5: Fallback handler catches unmatched routes

## Non-Goals

- No actual batch processing implementation
- No accurate tokenization (would require a tokenizer library)
- No Anthropic Files API endpoints
- No Anthropic Skills API endpoints

## Technical Considerations

- For static model mapping, consider a config file or env var listing supported models
- If proxying OpenAI models, translate model names (e.g., `gpt-4o` -> a mapped Anthropic-style name) or expose OpenAI names directly
- axum fallback handler for unknown routes: `Router::fallback()`
- count_tokens approximation: ~4 chars per token is a rough heuristic, clearly documented as approximate

## Success Metrics

- Integration tests for all three endpoints
- Unknown route test returns Anthropic-shaped 404
- `cargo test` passes
