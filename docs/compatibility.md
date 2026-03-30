# Compatibility Matrix

This document describes which backends, API endpoints, and features are supported by anyllm-proxy, along with their verification status and known limitations.

## Backend Support

| Backend | Status | Configuration | Notes |
|---------|--------|---------------|-------|
| OpenAI | Verified | `BACKEND=openai` (default)<br>`OPENAI_API_KEY`<br>`OPENAI_BASE_URL` (optional) | Primary backend; Chat Completions format; fully tested |
| Azure OpenAI | Integrated | `BACKEND=azure`<br>`AZURE_OPENAI_ENDPOINT`<br>`AZURE_OPENAI_DEPLOYMENT`<br>`AZURE_OPENAI_API_KEY`<br>`AZURE_OPENAI_API_VERSION` (optional) | Deployment-scoped URL; not tested against live API; use `cargo test --test live_azure -- --ignored --test-threads=1` |
| OpenAI Responses API | Integrated | `BACKEND=openai`<br>`OPENAI_API_FORMAT=responses` | Wired up but not tested against live API |
| Vertex AI (Google) | Verified | `BACKEND=vertex`<br>`VERTEX_PROJECT`<br>`VERTEX_REGION`<br>(`VERTEX_API_KEY` or `GOOGLE_ACCESS_TOKEN`) | Full translation; embeddings passthrough supported |
| Gemini (Google) | Verified | `BACKEND=gemini`<br>`GEMINI_API_KEY`<br>`GEMINI_BASE_URL` (optional) | Native `generateContent` path; non-streaming and streaming SSE with full-response diffing; embeddings passthrough with `gemini-embedding-exp-03-07` |
| AWS Bedrock | Integrated | `BACKEND=bedrock`<br>`AWS_REGION`<br>`AWS_ACCESS_KEY_ID`<br>`AWS_SECRET_ACCESS_KEY`<br>`AWS_SESSION_TOKEN` (optional) | SigV4 signed requests; Event Stream decoding for streaming; not tested against live API; use `cargo test --test live_bedrock -- --ignored --test-threads=1` |
| Anthropic (Passthrough) | Verified | `BACKEND=anthropic` | No translation; forwards requests as-is; `/v1/embeddings` not mounted |
| vLLM / HuggingFace | Passthrough | OpenAI-compatible Chat Completions | Embeddings passthrough supported via `POST /v1/embeddings` |

## API Endpoints

| Endpoint | Method | Input Format | Output Format | Status | Notes |
|----------|--------|--------------|---------------|--------|-------|
| `/v1/messages` | POST | Anthropic Messages API | Anthropic Messages API | Verified | Full translation; streaming SSE, tool calling, file/document blocks |
| `/v1/chat/completions` | POST | OpenAI Chat Completions | OpenAI Chat Completions | Verified | Unblocks OpenAI-native clients; uses `ReverseStreamingTranslator` for streaming |
| `/v1/embeddings` | POST | OpenAI embeddings request | OpenAI embeddings response | Verified | Passthrough only (no translation); works with OpenAI, Vertex, Gemini, vLLM/HuggingFace; not mounted for Anthropic passthrough |
| `/v1/models` | GET | - | Model list | Verified | Returns available models; enrichable via routing config |
| `/v1/messages/batches` | POST | Anthropic batch format | Anthropic batch format | Verified | Full CRUD (create, get, list, cancel, results); translated to/from OpenAI batch format |
| `/v1/messages/batches/{batch_id}` | GET | - | Batch status/result | Verified | Retrieve batch status or results |
| `/v1/messages/batches/{batch_id}/cancel` | POST | - | Batch status | Verified | Cancel in-progress batch |
| `/v1/messages/batches/{batch_id}/results` | GET | - | Batch results stream | Verified | Stream results as JSONL |
| `/health` | GET | - | Health status | Verified | Proxy health check; returns `{"status": "ok"}` |
| `/metrics` | GET | - | Prometheus metrics | Verified | Request counts, success/error rates |
| `POST /v1/completions/count_tokens` | POST | Token count request | Token count response | Partial | Approximate counts via tiktoken; not fully validated |

## Feature Translation

| Feature | Input | Output | Status | Notes |
|---------|-------|--------|--------|-------|
| System prompt | Anthropic `system` field | OpenAI `system` role or `developer` role (mapped) | Verified | Translates to appropriate role for backend |
| User messages | Anthropic `user` role | OpenAI `user` role | Verified | Direct mapping |
| Tool calling | Anthropic tool_use blocks | OpenAI tool_call objects | Verified | Tool IDs pass through directly; tool choice supports `auto`, `any`, `none`, and specific tools; `strict: true` set when `tool_choice: {type: "tool", name: "X"}` |
| Streaming SSE | Anthropic events | OpenAI chunks or Anthropic events | Verified | Bidirectional via state machines in `streaming_map.rs` and `reverse_streaming_map.rs` |
| File/document blocks | Anthropic `document` and `image` blocks | OpenAI `text` with model-specific content | Verified | Translates to native backend format |
| Thinking/reasoning blocks | Anthropic `thinking` blocks | `reasoning_content` on assistant message (DeepSeek/Qwen) | Verified | Bidirectional; thinking config budget_tokens dropped with warning |
| Cache control | Anthropic `cache_control` | Varies by backend | Partial | Stripped with `x-anyllm-degradation` warning when backend lacks support |
| Top_k | OpenAI `top_k` parameter | Varies by backend | Partial | Stripped with `x-anyllm-degradation` warning when backend lacks support |
| Stop sequences | Anthropic/OpenAI `stop` | Backend native | Partial | Truncated with warning if too many; signaled in `x-anyllm-degradation` |
| Batch API | Anthropic batch format | Varies by backend | Verified | Full CRUD; translated to OpenAI batch for compatible backends |

## Degradation Header

The `x-anyllm-degradation` response header is set when features are silently dropped during translation. Example values:

- `top_k` - top_k parameter not supported by backend
- `thinking_config` - thinking budget configuration dropped
- `cache_control` - cache control directives not supported
- `document_blocks` - document blocks converted to text
- `stop_sequences_truncated` - stop sequence list truncated

## Virtual Keys and Rate Limiting

| Feature | Status | Notes |
|---------|--------|-------|
| Virtual key generation | Verified | Keys prefixed with `sk-vk`; SHA-256 hashed in SQLite |
| Virtual key CRUD | Verified | Admin API `/admin/api/keys` (requires `X-CSRF-Token` header for state changes) |
| Per-key RPM limiting | Verified | Sliding window; returns 429 with `retry-after` on excess |
| Per-key TPM limiting | Verified | Sliding window; returns 429 with `retry-after` on excess |
| Admin rate limiting | Verified | Sliding window per source IP |
| Redis fail-open policy | Verified | `RATE_LIMIT_FAIL_POLICY=open` (default) allows requests when Redis unavailable; `closed` rejects with 503 |

## Advanced Features

| Feature | Status | Notes |
|---------|--------|-------|
| Cost tracking | Verified | `record_cost()` wired into all paths; costs in request logs and admin UI |
| Audit logging | Verified | SQLite `audit_log` table tracks all admin config mutations; includes `source_ip` |
| Spend alerts | Verified | Webhook notifications at 80%, 95%, and 100% of key budget |
| Model allowlist | Verified | Per-key policy with exact match and `prefix/*` wildcard support |
| Webhook callbacks | Verified | Fire-and-forget HTTP POST to configured URLs on request completion; SSRF-safe URL validation |
| OIDC discovery | Verified | SSRF-safe HTTP client; validates issuer and JWKS URLs against private IP ranges |
| CSRF protection | Verified | Admin state-mutating endpoints require `X-CSRF-Token` header (double-submit cookie pattern) |
| Per-entry cache TTL | Verified | `MemoryCache` enforces per-entry TTL via moka `Expiry` trait |
| IP allowlisting | Verified | CIDR ranges and bare IPs via `IP_ALLOWLIST`; `TRUST_PROXY_HEADERS` to use `X-Forwarded-For` |
| Langfuse integration | Verified | Native tracing when `LANGFUSE_PUBLIC_KEY` and `LANGFUSE_SECRET_KEY` set, or via config `callbacks: ["langfuse"]` |
| OpenTelemetry export | Verified | `--features otel` enables OTLP span export; zero overhead when feature is off |
| Request timeout | Verified | `REQUEST_TIMEOUT_SECS` limits wall-clock duration for streaming responses (default: 900, 0 = disabled) |
| Model pricing | Verified | Embedded pricing with `MODEL_PRICING_FILE` override option |

## Known Limitations

### Approximate Token Counting

The `count_tokens` endpoint uses tiktoken for approximate counts. It does not exactly match the token counts used by Claude or other providers for billing or context window calculations. For accurate counts, rely on the `usage` field in the response.

### Environment Variable Quoting

Multi-line values in `.env` files are not supported. Use environment variable expansion or config files for complex values.

### PROXY_OPEN_RELAY Development Mode

The `PROXY_OPEN_RELAY=true` setting accepts any non-empty API key without validation. This is insecure and intended only for local development. Always set `PROXY_API_KEYS` in production with a comma-separated list of allowed keys.

### MODEL_PRICING_FILE Format

The pricing file must be valid JSON in the following format:

```json
[
  {
    "model_pattern": "gpt-4*",
    "input_cost_per_token": 0.000015,
    "output_cost_per_token": 0.000045,
    "provider": "openai"
  }
]
```

If the file is unreadable or malformed, the proxy falls back to embedded pricing and logs a warning.

### Bedrock and Azure Live Testing

AWS Bedrock and Azure OpenAI backends are integrated but not included in default CI runs. To test against live APIs:

```bash
# Bedrock
AWS_REGION=us-east-1 AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... \
  cargo test --test live_bedrock -- --ignored --test-threads=1

# Azure OpenAI
AZURE_OPENAI_API_KEY=... cargo test --test live_azure -- --ignored --test-threads=1
```

### DeepSeek/Qwen Thinking Models

Thinking model support uses `reasoning_content` on the OpenAI side. The `thinking` config's `budget_tokens` parameter is not supported by standard OpenAI models and is dropped with a warning.

### Local LLM Compatibility

Tools with missing or empty IDs in streaming responses are automatically assigned synthetic `toolu_` IDs. Non-standard finish reasons (e.g., `insufficient_system_resource`) are mapped to `end_turn`.

## Build and Test

```bash
cargo build                          # Build all crates
cargo test                           # Run all tests (~906 passing, 8 ignored)
cargo clippy -- -D warnings          # Lint check
cargo fmt --check                    # Format check
```

For feature-specific testing:

```bash
cargo test -p anyllm_client          # Client library tests
cargo test -p anyllm_translate       # Translation logic tests
cargo test -p anyllm_proxy           # Proxy server tests
cargo test --test virtual_keys       # Virtual key + rate limit integration
```

## Security Hardening

As of the 2026-03-30 audit:

- `AWS_ACCESS_KEY_ID` and `GOOGLE_ACCESS_TOKEN` are redacted in `/admin/api/env` output
- Admin rate limiter uses sliding window (not fixed window)
- All audit log entries include `source_ip`
- OIDC discovery and webhook callback URLs are validated against private IP ranges using an SSRF-safe HTTP client
- Plaintext HTTP startup warning issued if listening on non-loopback without TLS
- 1MB body size limit on admin endpoints
- Content Security Policy (CSP) headers applied to admin UI
- Model names validated on input
