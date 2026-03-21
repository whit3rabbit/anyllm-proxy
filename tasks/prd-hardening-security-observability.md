# PRD: Phase 10 - Hardening, Security, Observability

## Introduction

Harden the proxy for production use: add retry logic with backoff for transient upstream errors, header filtering and secret redaction, structured logging with request ID correlation, metrics collection, SSRF protection for URL inputs, and concurrency limits. This phase turns a working proxy into a production-grade one.

## Goals

- Implement retry with exponential backoff for 429 and 5xx upstream errors
- Filter and redact sensitive headers from logs and forwarded requests
- Add request ID correlation across all log entries
- Add basic metrics (request count, latency, error rate)
- Protect against SSRF when handling URL-based inputs
- Add concurrency limits to prevent self-DoS

## User Stories

### US-001: Retry with backoff
**Description:** As an operator, I need the proxy to retry transient OpenAI errors instead of immediately failing, so brief upstream issues don't cascade to clients.

**Acceptance Criteria:**
- [ ] Retry on HTTP 429 (rate limited) and 5xx (server errors)
- [ ] Respect `Retry-After` header if present
- [ ] Exponential backoff: 1s, 2s, 4s (3 attempts max)
- [ ] Do NOT retry on 4xx (except 429) or client errors
- [ ] Do NOT retry streaming requests (only non-streaming)
- [ ] Log each retry attempt with attempt number and backoff duration
- [ ] Test: mock upstream returning 429 then 200, verify retry succeeds

### US-002: Header filtering
**Description:** As a security engineer, I need sensitive headers stripped from forwarded requests and responses so secrets don't leak across trust boundaries.

**Acceptance Criteria:**
- [ ] Inbound `x-api-key` is NOT forwarded to OpenAI
- [ ] Inbound `anthropic-version` and `anthropic-beta` are NOT forwarded
- [ ] OpenAI `Authorization` header is NOT included in response to client
- [ ] OpenAI rate limit headers (`x-ratelimit-*`) optionally translated to Anthropic format or stripped
- [ ] Test: verify no sensitive headers in outbound request or response

### US-003: Secret redaction in logs
**Description:** As an operator, I need API keys, auth tokens, and other secrets redacted in all log output so log aggregation is safe.

**Acceptance Criteria:**
- [ ] `Authorization: Bearer sk-...` redacted to `Authorization: Bearer sk-...REDACTED`
- [ ] `x-api-key` value redacted in any log line
- [ ] Request/response body logging (if enabled) redacts bearer tokens
- [ ] Redaction uses the `util/redact.rs` module from translator crate
- [ ] Test: log output does not contain raw API keys

### US-004: Request ID correlation
**Description:** As an operator debugging production issues, I need every log line for a request to include the same request ID so I can trace a request end to end.

**Acceptance Criteria:**
- [ ] Request ID (from Phase 6 middleware) is in every tracing span
- [ ] Log lines from upstream client call include the request ID
- [ ] Request ID sent to OpenAI as a custom header for upstream correlation
- [ ] Test: verify request ID appears in structured log output

### US-005: Metrics collection
**Description:** As an operator, I need basic request metrics so I can monitor proxy health and performance.

**Acceptance Criteria:**
- [ ] Track: total request count, request latency histogram, error count by status code
- [ ] Metrics stored in-memory (atomic counters / histogram)
- [ ] `GET /metrics` endpoint exposing current values (JSON or Prometheus format)
- [ ] Latency measured from request receipt to response completion
- [ ] Test: make requests, verify metrics reflect them

### US-006: SSRF protection
**Description:** As a security engineer, I need URL-based inputs (image URLs, file URLs) validated against SSRF before the proxy fetches them.

**Acceptance Criteria:**
- [ ] Block URLs pointing to private IP ranges (10.x, 172.16-31.x, 192.168.x, 127.x, ::1, link-local)
- [ ] Block URLs with non-HTTP(S) schemes
- [ ] Block URLs resolving to private IPs (DNS rebinding protection via resolution before fetch)
- [ ] Configurable allowlist for internal URLs if needed
- [ ] Test: private IP URL blocked, public URL allowed

### US-007: Concurrency limits
**Description:** As an operator, I need the proxy to limit concurrent requests so a burst of traffic doesn't exhaust connections or memory.

**Acceptance Criteria:**
- [ ] Configurable max concurrent requests (default: 256)
- [ ] Requests exceeding the limit -> 429 `rate_limit_error` with `Retry-After` header
- [ ] Uses tower `ConcurrencyLimit` or semaphore
- [ ] Test: exceed limit, verify 429 response

## Functional Requirements

- FR-1: Retry logic only applies to non-streaming requests
- FR-2: Header filtering is applied in both directions (client->proxy and proxy->client)
- FR-3: Secret redaction covers all log levels (debug, info, warn, error)
- FR-4: Metrics endpoint does not require authentication
- FR-5: SSRF protection applies to any URL extracted from request content
- FR-6: Concurrency limit applies to all routes except `/health` and `/metrics`

## Non-Goals

- No distributed tracing (OpenTelemetry export)
- No rate limiting per API key (simple concurrency limit only)
- No mutual TLS
- No audit logging to external systems
- No WAF-style request inspection

## Technical Considerations

- Use `tower::retry` or manual retry loop with `tokio::time::sleep`
- Parse `Retry-After` as either seconds (integer) or HTTP date
- For SSRF: resolve DNS before connecting, check resolved IPs against blocklist
- Metrics: consider `metrics` crate with `metrics-exporter-prometheus` or simple atomic counters
- Concurrency: `tower::limit::ConcurrencyLimitLayer` integrates cleanly with axum

## Success Metrics

- Retry test: 429 followed by 200 succeeds without client seeing the 429
- Header filtering test: no sensitive headers leak
- SSRF test: private IP URLs rejected
- Concurrency test: limit enforced correctly
- `cargo clippy -- -D warnings` passes
- `cargo test` passes
