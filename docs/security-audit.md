# Security Audit

## Authentication

### Proxy Auth Boundary
- The proxy validates that requests carry either `x-api-key` or `Authorization: Bearer` header
- The proxy does NOT verify the key value itself; it prevents accidental open proxying
- Client credentials are never forwarded to OpenAI
- OpenAI is always called with the server-configured `OPENAI_API_KEY`

### Recommendation
- For production, add key verification against an allowlist
- Consider mTLS or OAuth/JWT at the proxy edge for multi-tenant deployments

## Request Size Limits

- 32 MB body limit enforced via axum's `DefaultBodyLimit`
- Matches Anthropic's documented Messages endpoint limit
- Prevents memory exhaustion from oversized payloads

## SSRF Prevention

- The proxy only makes outbound HTTP requests to `OPENAI_BASE_URL` (configured via env)
- No user-controlled URLs are fetched:
  - Image URLs in content are passed as text, not fetched
  - Document blocks are converted to text notes, not processed
  - No file download/upload proxying

### Recommendation
- Validate `OPENAI_BASE_URL` is not a private/internal address at startup
- Consider URL allowlist for production

## Secret Handling

- `OPENAI_API_KEY` is read from environment (never hardcoded)
- `redact_secret()` utility available for logging (shows first/last 4 chars only)
- Authorization headers are not logged
- Request bodies may contain sensitive content; `RUST_LOG` should be `info` in production

## Concurrency Protection

- Tower `ConcurrencyLimitLayer` caps at 100 concurrent requests
- Prevents self-DOS during upstream 429 incidents
- Retry logic has exponential backoff to avoid hammering upstream

## Header Filtering

- Inbound `x-api-key` and `Authorization` headers are consumed, not forwarded
- `anthropic-version` header is accepted but not forwarded
- Outbound requests only include `Authorization: Bearer` with server key
- `x-request-id` is generated/echoed for correlation

## Streaming Security

- Bounded channel (capacity 32) prevents unbounded memory growth
- Client disconnect detected and upstream connection dropped
- No content buffering; deltas translated directly

## Dependencies

- All dependencies are from crates.io (auditable)
- No `unsafe` code in project source
- `cargo audit` should be run in CI

## OWASP Top 10 Relevance

| Risk | Status | Notes |
|---|---|---|
| Injection | Mitigated | No SQL/shell; JSON parsed via serde |
| Broken Auth | Partial | Auth presence checked; value not verified |
| Sensitive Data Exposure | Mitigated | Secrets redacted in logs; env-based config |
| XML External Entities | N/A | JSON only |
| Broken Access Control | N/A | Single-purpose proxy |
| Security Misconfiguration | Mitigated | Sensible defaults; env-based config |
| XSS | N/A | API-only, no HTML |
| Insecure Deserialization | Mitigated | Typed serde with strict schemas |
| Using Components with Known Vulns | Recommendation | Run cargo audit in CI |
| Insufficient Logging | Mitigated | Structured tracing with request IDs |
