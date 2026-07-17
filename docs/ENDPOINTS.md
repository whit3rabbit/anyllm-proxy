# Endpoint Reference

Complete reference for every HTTP endpoint exposed by **anyllm-proxy**. The proxy runs two servers:

- **Proxy server** — default port 3000, configurable via `LISTEN_PORT`. All client API traffic.
- **Admin server** — default port 3001, localhost-only. Management and observability.

---

## Contents

- [Global constraints](#global-constraints)
- [Authentication](#authentication)
- [Backend modes](#backend-modes)
- [Proxy server — public endpoints](#proxy-server--public-endpoints)
- [Proxy server — API endpoints](#proxy-server--api-endpoints)
  - [Anthropic Messages API](#anthropic-messages-api)
  - [OpenAI Chat Completions](#openai-chat-completions)
  - [Gemini input compatibility](#gemini-input-compatibility)
  - [Models](#models)
  - [Embeddings, audio, images, completions, rerank](#embeddings-audio-images-completions-rerank)
  - [Files](#files)
  - [Batch jobs (OpenAI format)](#batch-jobs-openai-format)
  - [Anthropic batch API](#anthropic-batch-api)
  - [Bedrock native endpoints](#bedrock-native-endpoints)
  - [Generic /v1/\* passthrough (catch-all)](#generic-v1-passthrough-catch-all)
- [Named backend routing](#named-backend-routing)
- [Admin server endpoints](#admin-server-endpoints)

---

## Global constraints

| Constraint | Value |
|---|---|
| Max request body | 32 MB (proxy), 1 MB (admin) |
| Max concurrent requests | 100 per proxy instance (429 when exceeded, no queuing) |
| Concurrency permit | Held until full response completes — important for streaming |
| Request ID | Auto-generated and injected if `x-request-id` is absent |

---

## Authentication

Every proxy API endpoint except `/health` requires authentication.

### Supported auth methods

| Method | How |
|---|---|
| Bearer token | `Authorization: Bearer <key>` |
| Legacy API key | `x-api-key: <key>` |
| OIDC/JWT | `Authorization: Bearer <jwt>` when `OIDC_ISSUER_URL` is set |
| Virtual key | Same bearer format; enables per-key model allowlists and spend tracking |

Unauthenticated requests return `401 Unauthorized` with an Anthropic-shaped error body.

### Loopback-open default

When **no** proxy auth is configured (no `PROXY_API_KEYS`, no `PROXY_OPEN_RELAY=true`, no virtual keys, no OIDC), the proxy accepts unauthenticated requests **from loopback (localhost) peers only**; LAN/remote peers still get `401`. This makes local dev work out of the box while keeping the port closed to the network. The decision uses the real TCP peer address (`ConnectInfo`), not the client-spoofable `X-Forwarded-For`.

Caveat: behind a reverse proxy running on localhost, every request appears to come from loopback, so the proxy is effectively open. Set `PROXY_API_KEYS` in that topology. The effective posture is reported as `auth_mode` (`keys` / `open_relay` / `loopback_only`) by `GET /admin/api/status` and surfaced as a warning banner in the admin UI.

### IP allowlist

Optional. Set `IP_ALLOWLIST=<cidr,...>` to reject any source IP not in the list (403). Applied before auth.

---

## Backend modes

A backend is selected per request based on configuration. The mode affects which endpoints are available.

| Mode | When | Description |
|---|---|---|
| **Translate** | `BACKEND=openai` (default), `azure`, `vertex`, `gemini` (OpenAI-compat) | Full endpoint set; translates Anthropic ↔ OpenAI |
| **Anthropic** | `BACKEND=anthropic` | Passthrough — forwards Anthropic format as-is to `api.anthropic.com` |
| **Bedrock** | `BACKEND=bedrock` | SigV4 signing; Anthropic format or native Bedrock format |
| **GeminiNative** | `BACKEND=gemini` with `GEMINI_API_FORMAT=native` | Sends Gemini native format; no OpenAI translation |

---

## Proxy server — public endpoints

### `GET /health`

Health check. No authentication required.

```
200 OK
{"status":"ok"}
```

---

## Proxy server — API endpoints

All endpoints below require authentication (see [Authentication](#authentication)).

---

### Anthropic Messages API

#### `POST /v1/messages`

Create a message. Supports streaming via `"stream": true`.

**Supported modes:** All (Translate, Anthropic, Bedrock, GeminiNative)

**Request headers (optional):**

| Header | Description |
|---|---|
| `anthropic-beta` | Beta feature flags (forwarded to Anthropic backend as-is) |
| `x-claude-code-session-id` | Session correlation ID (forwarded to Anthropic backend) |

**Request body:** `anthropic::MessageCreateRequest`

Key fields:

| Field | Type | Notes |
|---|---|---|
| `model` | string | Required. Mapped to backend model via model router |
| `messages` | array | Required. `[{"role": "user\|assistant", "content": ...}]` |
| `max_tokens` | integer | Required |
| `system` | string\|array | Optional system prompt |
| `stream` | boolean | `false` default |
| `tools` | array | Tool definitions |
| `tool_choice` | object | Tool selection strategy |
| `temperature`, `top_p`, `top_k` | number | Sampling params |
| `thinking` | object | Extended thinking config (Anthropic models only) |

**Response (non-streaming):** `anthropic::MessageResponse`

**Response (streaming):** SSE events

| Event | Description |
|---|---|
| `message_start` | Message object with `usage.input_tokens` |
| `content_block_start` | Start of a content block |
| `content_block_delta` | Incremental text or tool input delta |
| `content_block_stop` | End of a content block |
| `message_delta` | Stop reason and output token count |
| `message_stop` | Stream end |

**Response headers (Translate/Bedrock mode):**

| Header | Description |
|---|---|
| `x-anyllm-cache` | `miss` or `bypass` — cache status |
| `x-anyllm-degradation` | Features dropped during translation (if `expose_degradation_warnings` enabled) |
| `x-ratelimit-*` | Rate limit info forwarded from upstream (OpenAI format) |

**Virtual key enforcement:** Model allowlist checked against `model` field. Requests with disallowed models return `403 Forbidden`.

---

#### `POST /v1/messages/count_tokens`

Estimate token count for a request. Does not call the backend.

**Supported modes:** Translate only

**Request body:** Same as `POST /v1/messages`

**Response:**

```json
{"input_tokens": 42}
```

**Response headers:**

| Header | Value |
|---|---|
| `x-anyllm-token-counter` | `approximate (tiktoken o200k_base); do not use for billing` |

> Token counting uses tiktoken's `o200k_base` encoding (GPT-4o). Results are approximate and not equivalent to Anthropic's tokenizer. Do not use for billing.

---

### Anthropic batch API

#### `POST /v1/messages/batches`

Create an Anthropic-format batch job. Translates to OpenAI batch internally.

**Supported modes:** Translate (OpenAI, AzureOpenAI backends only)

**Request body:**

```json
{
  "requests": [
    {
      "custom_id": "req-1",
      "params": { /* same as POST /v1/messages */ }
    }
  ]
}
```

Constraints:
- All requests in the batch must use the same `model`
- Virtual key model allowlist enforced per request item

**Response:** Anthropic `MessageBatch` object

---

#### `GET /v1/messages/batches/{id}`

Get status of an Anthropic batch.

**Supported modes:** Translate (OpenAI, AzureOpenAI backends only)

**Response:** Anthropic `MessageBatch` object

---

#### `GET /v1/messages/batches/{id}/results`

Get results of a completed Anthropic batch.

**Supported modes:** Translate (OpenAI, AzureOpenAI backends only)

**Response:** `application/x-jsonl` — one JSON object per line, each with `custom_id` and Anthropic `Message`

---

### OpenAI Chat Completions

#### `POST /v1/chat/completions`

OpenAI Chat Completions format. Translates to Anthropic internally and back.

**Supported modes:** Translate only

**Request body:** `openai::ChatCompletionRequest`

Key fields:

| Field | Type | Notes |
|---|---|---|
| `model` | string | Required |
| `messages` | array | `[{"role": "...", "content": ...}]` |
| `stream` | boolean | |
| `tools` | array | OpenAI tool definitions |
| `tool_choice` | string\|object | |
| `temperature`, `top_p`, `max_tokens` | | |
| `reasoning_effort` | string | Maps to Anthropic thinking blocks |

Unknown fields are passed through via `serde_json::Map` (flattened `extra`).

**Response (non-streaming):** `openai::ChatCompletionResponse`

**Response (streaming):** SSE with `data: {...}` chunks; ends with `data: [DONE]`

**Response headers:**

| Header | Description |
|---|---|
| `x-anyllm-cache` | Cache status |
| `x-anyllm-degradation` | Translation degradation warnings (if enabled) |

---

### Gemini input compatibility

#### `POST /v1beta/models/{model_action}`

Accept Gemini native format from `gemini-cli` and translate to Anthropic internally.

**Supported modes:** All backends

`model_action` format:
- `{model}:generateContent` — non-streaming
- `{model}:streamGenerateContent` — streaming SSE
- `{model}:countTokens` — local token count, no backend call; returns `{"totalTokens": N}`

**Request body:** `GenerateContentRequest` (Gemini native format)

**Response:**
- Non-streaming: `GenerateContentResponse`
- Streaming: SSE with Gemini-format events

**Use case:** Point `GEMINI_BASE_URL` at this proxy to route Gemini CLI requests through any backend without changing client code.

---

### Models

#### `GET /v1/models`

List available models.

**Supported modes:** All backends

**Response:**

```json
{
  "object": "list",
  "data": [
    {"id": "claude-opus-4-6", "object": "model", "created": 1715644800, "owned_by": "anthropic"},
    ...
  ]
}
```

Returns static Claude model entries merged with any dynamically configured models from the model router.

---

### Embeddings, audio, images, completions, rerank

These are forwarded to the backend unchanged (passthrough). No Anthropic↔OpenAI translation.

**Supported modes:** Translate only

#### `POST /v1/embeddings`

Text embeddings. Request and response forwarded as-is.

#### `POST /v1/audio/transcriptions`

Audio transcription. Accepts `multipart/form-data` with audio file.

#### `POST /v1/audio/speech`

Text-to-speech. JSON request body, binary audio response (mp3/opus/aac/flac/pcm).

#### `POST /v1/images/generations`

Image generation. JSON passthrough.

#### `POST /v1/rerank`

Reranking (Cohere v1 format). JSON passthrough.

#### `POST /v2/rerank`

Reranking (Cohere v2 format). JSON passthrough. Path forwarded verbatim to the backend.

#### `POST /v1/completions`

Legacy completions API. JSON passthrough.

---

### Files

#### `POST /v1/files`

Upload a file for batch jobs or other purposes.

**Supported modes:** All backends (handled by batch engine)

> File operations beyond upload (list, retrieve, delete) are only available in Translate mode via the generic `/v1/*` catch-all, or in Anthropic mode via the Anthropic-native catch-all. Bedrock and GeminiNative modes only support upload.

**Request:** `multipart/form-data`

| Field | Type | Description |
|---|---|---|
| `file` | binary | File content (JSONL for batches) |
| `purpose` | string | `"batch"` (required) |

**Response:**

```json
{
  "id": "file-abc123",
  "object": "file",
  "bytes": 1024,
  "created_at": 1700000000,
  "filename": "batch.jsonl",
  "purpose": "batch"
}
```

---

### Batch jobs (OpenAI format)

#### `POST /v1/batches`

Create a batch job.

**Supported modes:** OpenAI, AzureOpenAI backends

**Request body:**

```json
{
  "input_file_id": "file-abc123",
  "endpoint": "/v1/chat/completions",
  "completion_window": "24h",
  "metadata": {"key": "value"},
  "webhook_url": "https://example.com/webhook"
}
```

`webhook_url` is validated against SSRF: private, loopback, and metadata service IPs are rejected.

**Response:** Batch job object

---

#### `GET /v1/batches`

List batch jobs.

**Query parameters:**

| Param | Description |
|---|---|
| `limit` | Max results per page (max 100, default 20) |
| `after` | Pagination cursor (batch ID) |

**Response:**

```json
{
  "object": "list",
  "data": [...],
  "has_more": false,
  "first_id": "batch-...",
  "last_id": "batch-..."
}
```

---

#### `GET /v1/batches/{batch_id}`

Get a batch job by ID.

---

#### `POST /v1/batches/{batch_id}/cancel`

Cancel a running batch job.

---

### Bedrock native endpoints

Available only when `BACKEND=bedrock`. Clients send Bedrock-native JSON; the proxy handles SigV4 signing.

These routes are mounted at `/model/{modelId}/...` (or `/{backend_name}/model/{modelId}/...` for named backends).

#### `POST /model/{modelId}/converse`

Bedrock Converse API — standardized multi-turn chat format.

**Request body:** AWS Bedrock `ConverseRequest`

Key fields:

| Field | Description |
|---|---|
| `messages` | Array of `{"role": "user\|assistant", "content": [...]}` |
| `system` | System prompt array |
| `inferenceConfig` | `{maxTokens, temperature, topP, stopSequences}` |
| `toolConfig` | Tool definitions |
| `guardrailConfig` | Optional Bedrock guardrail settings |

**Response:** AWS Bedrock `ConverseResponse`

```json
{
  "output": {"message": {"role": "assistant", "content": [{"text": "..."}]}},
  "stopReason": "end_turn",
  "usage": {"inputTokens": 10, "outputTokens": 25, "totalTokens": 35}
}
```

---

#### `POST /model/{modelId}/converse-stream`

Bedrock Converse API with streaming. Returns AWS Event Stream binary frames.

Same request format as `/converse`. Response is the raw AWS Event Stream framing.

---

#### `POST /model/{modelId}/invoke`

Bedrock InvokeModel — model-native JSON format. Use for models with model-specific schemas (e.g., Stable Diffusion, Titan, etc.).

**Request body:** Model-specific JSON (no standardized schema)

**Response body:** Model-specific JSON

---

#### `POST /model/{modelId}/invoke-with-response-stream`

Streaming variant of InvokeModel. Returns AWS Event Stream binary frames.

---

### Generic `/v1/*` passthrough (catch-all)

#### `ANY /v1/{*path}`

Catch-all for any `/v1/` path without an explicit handler. Registered last so explicit routes take priority.

**Supported modes:** Translate only (OpenAI-compatible backends)

**HTTP methods:** All (GET, POST, PUT, DELETE, PATCH, etc.)

**Request:** Forwarded as-is (body, content-type, query string)

**Response:** Streamed back as-is (JSON, binary, SSE all work)

**Headers forwarded from client:**

| Header | Forwarded |
|---|---|
| `content-type` | Yes |
| `openai-beta` | Yes (via generic proxy) |
| `anthropic-beta` | Yes (via generic proxy) |
| `authorization` | No (replaced with backend credentials) |
| `host` | No (set by reqwest) |

**Hop-by-hop headers stripped from response:** `transfer-encoding`, `connection`, `keep-alive`, `proxy-authenticate`, `proxy-authorization`, `te`, `trailer`, `upgrade`

**Endpoints this covers (non-exhaustive):**

| Path pattern | Description |
|---|---|
| `POST /v1/responses` | OpenAI Responses API |
| `GET/DELETE /v1/files/{id}` | File retrieval/deletion |
| `GET /v1/files/{id}/content` | Download file content |
| `POST /v1/moderations` | Content moderation |
| `POST /v1/images/edits` | Image editing (multipart) |
| `POST /v1/images/variations` | Image variations (multipart) |
| `POST /v1/videos` | Video generation |
| `GET /v1/videos/{id}` | Video status polling |
| `GET /v1/videos/{id}/content` | Download video |
| `/v1/fine_tuning/jobs` + sub-paths | Fine-tuning |
| `/v1/evals` + sub-paths | Evaluations |
| `/v1/assistants` + sub-paths | Assistants API (deprecated Aug 2026) |
| `/v1/threads` + sub-paths | Threads/runs (Assistants API) |
| `/v1/containers` + sub-paths | Code interpreter containers |
| `/v1/vector_stores` + sub-paths | Vector stores |
| `/v1/ocr` | OCR (Mistral, Azure, Vertex) |
| `/v1/search/{provider}` | Web search providers |
| `/v1/skills` | Anthropic Skills API |

> The backend must natively support the endpoint. For example, `/v1/vector_stores` only works if the backend is OpenAI or a compatible provider.

---

## Named backend routing

All proxy endpoints are available under a named backend prefix:

```
/{backend_name}/v1/messages
/{backend_name}/v1/chat/completions
/{backend_name}/model/{modelId}/converse
...
```

The default backend is also served without a prefix for backward compatibility.

Named backends are configured in the YAML config file. See `docs/CONFIG.md`.

---

## Admin server endpoints

The admin server runs on `localhost:3001` by default (configurable via `ADMIN_BIND` and `ADMIN_PORT`). Docker deployments must set `ADMIN_BIND=0.0.0.0`.

All admin API endpoints require a Bearer token (`Authorization: Bearer <admin-token>`).

Mutating endpoints (POST, PUT, DELETE, PATCH) also require:
- `X-CSRF-Token: <token>` header matching the value from `GET /admin/csrf-token`
- Origin/Host must be localhost

Rate limit: 10 requests per minute per source IP (configurable).

### Public (no auth)

#### `GET /admin/health`

```json
{"status": "ok"}
```

#### `GET /admin/csrf-token`

Returns a CSRF token for use in subsequent mutating requests.

**Response:**

```json
{"csrf_token": "..."}
```

Also sets cookie: `csrf_token=...; Path=/admin; SameSite=Strict; Max-Age=86400`

#### `GET /admin`, `GET /admin/`

Serve the embedded admin SPA. HTML with per-request CSP nonce.

---

### Config

#### `GET /admin/api/config`

Get current runtime configuration.

#### `PUT /admin/api/config`

Update runtime configuration. CSRF required.

#### `GET /admin/api/config/overrides`

List active configuration overrides.

#### `DELETE /admin/api/config/overrides/{key}`

Remove a configuration override. CSRF required.

#### `GET /admin/api/env`

Get environment variables. Secret values are redacted.

#### `POST /admin/api/env/import`

Import environment variables. CSRF required.

#### `GET /admin/api/env/export`

Export environment as bash-compatible format.

---

### Keys

#### `POST /admin/api/keys`

Create a virtual API key. CSRF required.

**Request body:**

```json
{
  "name": "my-key",
  "description": "optional description",
  "allowed_models": ["claude-opus-4-6", "claude-sonnet-4-6"]
}
```

`allowed_models` is optional. If omitted, all models are permitted.

**Response:** Key object including the generated credential (only shown once).

#### `GET /admin/api/keys`

List all virtual API keys. Credentials are redacted.

#### `PUT /admin/api/keys/{id}`

Update key metadata (name, description, allowed_models). CSRF required.

#### `DELETE /admin/api/keys/{id}`

Revoke a key. CSRF required.

#### `GET /admin/api/keys/{id}/spend`

Get cost and token usage summary for a key.

---

### Models

#### `GET /admin/api/models`

List configured models.

#### `POST /admin/api/models`

Add a model. CSRF required.

#### `POST /admin/api/models/discover`

Auto-discover available models from a backend provider. CSRF required.

#### `DELETE /admin/api/models/{name}`

Remove a model. CSRF required.

---

### Backends

#### `GET /admin/api/backends`

List configured backends with per-backend metrics (requests_total, requests_success, requests_error).

---

### MCP servers

#### `GET /admin/api/mcp-servers`

List configured MCP servers.

#### `POST /admin/api/mcp-servers`

Add an MCP server. CSRF required.

#### `DELETE /admin/api/mcp-servers/{name}`

Remove an MCP server. CSRF required.

---

### Observability

#### `GET /admin/api/metrics`

Aggregated proxy request metrics.

#### `GET /admin/api/observability/overview`

Dashboard overview: uptime, request rates, error rates, p50/p95 latency.

#### `GET /admin/api/requests`

Paginated request log.

**Query parameters:**

| Param | Description |
|---|---|
| `since` | RFC 3339 timestamp — return entries after this time |
| `until` | RFC 3339 timestamp — return entries before this time |
| `limit` | Max results per page |

#### `GET /admin/api/requests/{id}`

Get a single request log entry.

#### `GET /admin/api/audit`

Audit log. Records key creation/revocation, model changes, config changes.

**Query parameters:** `since`, `until`, `action` filter

---

### Status

#### `GET /admin/api/status`

Proxy health status (healthy, degraded).

#### `GET /admin/api/traffic`

Real-time traffic statistics.

#### `GET /admin/api/uptime`

Uptime percentage and statistics.

---

### WebSocket

#### `GET /admin/ws` (WebSocket upgrade)

Real-time server events. Authentication is passed as the first message after connection (browsers cannot set `Authorization` headers on WebSocket connections).

---

## Metrics endpoint

#### `GET /metrics`

Backend request metrics. Requires authentication.

**Response:**

```json
{
  "backends": {
    "default": {
      "requests_total": 1000,
      "requests_success": 990,
      "requests_error": 10
    }
  },
  "total": {
    "requests_total": 1000,
    "requests_success": 990,
    "requests_error": 10
  }
}
```

---

## Backend mode × endpoint matrix

| Endpoint | Translate | Anthropic | Bedrock | GeminiNative |
|---|---|---|---|---|
| `POST /v1/messages` | ✓ | ✓ | ✓ | ✓ |
| `POST /v1/messages/count_tokens` | ✓ | | | |
| `POST /v1/messages/batches` | ✓ (OpenAI/Azure) | | | |
| `GET /v1/messages/batches/{id}` | ✓ (OpenAI/Azure) | | | |
| `GET /v1/messages/batches/{id}/results` | ✓ (OpenAI/Azure) | | | |
| `POST /v1/chat/completions` | ✓ | | | |
| `POST /v1beta/models/{action}` | ✓ | ✓ | ✓ | ✓ |
| `GET /v1/models` | ✓ | ✓ | ✓ | ✓ |
| `POST /v1/embeddings` | ✓ | | | |
| `POST /v1/audio/transcriptions` | ✓ | | | |
| `POST /v1/audio/speech` | ✓ | | | |
| `POST /v1/images/generations` | ✓ | | | |
| `POST /v1/rerank` | ✓ | | | |
| `POST /v2/rerank` | ✓ | | | |
| `POST /v1/completions` | ✓ | | | |
| `POST /v1/files` (upload) | ✓ | ✓ | ✓ | ✓ |
| `GET/DELETE /v1/files/{id}` | ✓ (catch-all) | ✓ (catch-all) | | |
| `GET /v1/batches` | ✓ | ✓ | ✓ | ✓ |
| `GET/POST /v1/batches/{id}` | ✓ | ✓ | ✓ | ✓ |
| `ANY /v1/{*path}` (catch-all) | ✓ | ✓ | | |
| `POST /model/{id}/converse` | | | ✓ | |
| `POST /model/{id}/converse-stream` | | | ✓ | |
| `POST /model/{id}/invoke` | | | ✓ | |
| `POST /model/{id}/invoke-with-response-stream` | | | ✓ | |

---

## Not yet implemented (deferred to future work)

These endpoint categories require significant infrastructure not present in the proxy today:

| Endpoint | Reason deferred |
|---|---|
| `GET/POST /v1/realtime` (WebSocket) | WebSocket upgrade, bidirectional streaming, per-session state |
| `/mcp` (client-facing MCP gateway) | MCP JSON-RPC server, SSE transport, OAuth2/PKCE, tool aggregation |
| `/a2a` (A2A gateway) | A2A JSON-RPC 2.0 protocol, agent registry |
| `/rag/ingest`, `/rag/query` | OCR + chunking + embedding + vector store pipeline |
| `/guardrails/apply_guardrail` | Guardrail engine (Presidio, Bedrock Guardrails, etc.) |
| `/v1beta/interactions` | Google Interactions API bridge |

For OpenAI-compatible backends, all of these paths except the WebSocket/MCP/A2A ones are forwarded by the generic catch-all to the backend — so they work if the backend natively supports them.
