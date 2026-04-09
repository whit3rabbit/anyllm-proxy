# Proxy Architecture

## Crate Structure

Cargo workspace with five crates:

### `crates/providers` (lib: `anyllm_providers`)
Metadata-only catalog: no HTTP, no IO. `ProviderDef` (protocol, auth, env vars, LiteLLM prefix) and `ModelDef` (context window, capabilities). Registry functions in `registry.rs`. Add a new provider: create `providers/src/providers/<name>.rs`, register in `providers/mod.rs` and `registry.rs`. OpenAI-compatible providers route through the existing `OpenAIClient` automatically.

### `crates/client` (lib: `anyllm_client`)
Async HTTP client (Anthropic-in, Anthropic-out). `ClientBuilder`, `ToolBuilder`, `messages_stream()` returning `impl Stream`.

### `crates/translator` (lib: `anyllm_translate`)
Pure translation logic, no IO. Stateless `fn(A) -> B` mapping between Anthropic and OpenAI types.
- `anthropic/`: Anthropic Messages API types
- `openai/`: OpenAI types (Chat Completions + Responses API)
- `mapping/`: Conversion functions (message_map, tools_map, streaming_map, reverse_streaming_map, responses_*, warnings)
- `middleware/`: Request/response handler orchestrating translation

### `crates/batch_engine` (lib: `anyllm_batch_engine`)
HTTP-agnostic batch orchestration: job queue, file storage, webhook delivery.

### `crates/proxy` (bin: `anyllm_proxy`)
HTTP proxy on axum + reqwest:
- `server/`: Routes, middleware (auth, rate limit, request ID, size/concurrency limits), SSE streaming, passthrough handlers. `bedrock_native.rs`: Bedrock Converse/InvokeModel native passthrough (SigV4 handled by proxy). `generic_passthrough.rs`: catch-all `/v1/{*path}` for Translate mode (registered last).
- `backend/`: `BackendClient` enum dispatching to OpenAI/Azure/Vertex/Gemini/Anthropic/Bedrock with retry
- `admin/`: Admin server (localhost:3001), virtual key CRUD, managed backend CRUD (`routes/managed_backends.rs`), model management, audit log, WebSocket live updates
- `admin-ui/`: React 19 + TypeScript SPA (Vite). Build: `cd crates/proxy/admin-ui && npm run build`

## Data Flow

```
Client (Anthropic SDK) -> POST /v1/messages
  -> Auth middleware (validate x-api-key or Bearer)
  -> Request ID middleware (generate/echo x-request-id)
  -> Body size limit (32MB via DefaultBodyLimit)
  -> Concurrency limit (100 via tower ConcurrencyLimitLayer)
  -> Route handler
    -> Translate: Anthropic request -> OpenAI request
    -> OpenAI client (reqwest with retry/backoff)
    -> Translate: OpenAI response -> Anthropic response
  -> Client receives Anthropic-format response
```

## Header Rules

### Inbound (from client)
| Header | Required | Action |
|---|---|---|
| x-api-key | One of these | Validated for presence only |
| Authorization: Bearer ... | One of these | Validated for presence only |
| anthropic-version | No | Accepted but not forwarded |
| content-type | Yes | Must be application/json |
| x-request-id | No | Echoed; generated if absent |

### Outbound (to OpenAI)
| Header | Value | Notes |
|---|---|---|
| Authorization | Bearer {OPENAI_API_KEY} | From config, never from client |
| Content-Type | application/json | Set by reqwest |

### Response (to client)
| Header | Value |
|---|---|
| x-request-id | Request correlation ID |
| content-type | application/json or text/event-stream |

## Error Shape Translation

All errors returned to clients use Anthropic format:
```json
{
  "type": "error",
  "error": {
    "type": "invalid_request_error",
    "message": "..."
  }
}
```

OpenAI error status codes are mapped:
- 400 -> invalid_request_error
- 401 -> authentication_error
- 403 -> permission_error
- 404 -> not_found_error
- 429 -> rate_limit_error
- 500-502 -> api_error
- 503, 529 -> overloaded_error

## Retry Policy

- Retries on 429 and 5xx status codes
- Maximum 3 retries
- Exponential backoff: 500ms * 2^attempt + 25% jitter
- Respects retry-after header when present
- Each retry logged at WARN level

## Security

- Auth boundary: proxy never forwards client credentials to OpenAI
- SSRF prevention: only connects to configured OPENAI_BASE_URL
- Secret redaction utility for logging (shows first/last 4 chars)
- 32MB body size limit enforced at proxy edge
- 100 concurrent request limit
