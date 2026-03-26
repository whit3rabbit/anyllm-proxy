# Research: LiteLLM Gap Fill

**Date**: 2026-03-25 | **Branch**: `20260325-120000-litellm-gap-fill`

---

## 1. OpenAI Chat Completions Input Endpoint (R1)

### Decision
New handler at `POST /v1/chat/completions` with new mapping functions in the translator crate. Not middleware, not a new backend variant.

### Rationale
- All OpenAI types (`ChatCompletionRequest`, `ChatCompletionResponse`, `ChatCompletionChunk`) already exist in the codebase.
- Several reverse mapping functions already exist: `openai_tools_to_anthropic`, `openai_tool_choice_to_anthropic`, `anthropic_to_openai_usage`.
- A dedicated handler isolates OpenAI compat from the Anthropic-in pipeline.

### What exists (reusable)
| Function | Direction |
|---|---|
| `openai_tools_to_anthropic` | OpenAI -> Anthropic (exists) |
| `openai_tool_choice_to_anthropic` | OpenAI -> Anthropic (exists) |
| `anthropic_to_openai_usage` | Anthropic -> OpenAI (exists) |
| `StreamingTranslator` | OpenAI chunks -> Anthropic events (exists, forward only) |

### What must be written
| Function | Location |
|---|---|
| `openai_to_anthropic_request` | `mapping/message_map.rs` |
| `anthropic_to_openai_response` | `mapping/message_map.rs` |
| `ReverseStreamingTranslator` | `mapping/reverse_streaming_map.rs` (new file) |
| `POST /v1/chat/completions` handler | `server/chat_completions.rs` (new file) |

### Key field mappings (request)
- `messages[role=system]` -> `system` field
- `messages[role=user/assistant]` -> Anthropic `messages[]`
- `messages[role=tool]` -> Anthropic `tool_result` blocks
- `tool_calls` -> `tool_use` blocks (`arguments` JSON string -> `input` JSON object)
- `max_tokens` / `max_completion_tokens` -> `max_tokens` (required in Anthropic; reject 400 if absent)
- `stop` -> `stop_sequences`

### Lossy fields (drop with `x-anyllm-degradation`)
`presence_penalty`, `frequency_penalty`, `response_format`, `logprobs`, `n`, `seed`, `stream_options`

### Streaming reverse mapping
Anthropic `StreamEvent` -> OpenAI `ChatCompletionChunk`. The reverse translator tracks message ID, model, tool_call index. OpenAI has no `content_block_start/stop` envelope; tool calls use array index.

### Open design decision
`max_tokens` is required in Anthropic but optional in OpenAI. Options: (a) reject 400 if absent, (b) supply configurable default (e.g., 4096). Recommend (a) for correctness.

---

## 2. AWS Bedrock Backend (R2)

### Decision
Use `aws-sigv4` v1.4 + `aws-credential-types` v1.2 for minimal SigV4 signing. No full AWS SDK.

### Rationale
- `aws-sigv4` adds ~20-30 transitive crates vs ~80-120 for `aws-sdk-bedrockruntime`.
- The project uses reqwest for all HTTP; the full SDK would introduce a parallel hyper-based HTTP stack.
- Manual credential loading from env vars avoids pulling in `aws-config`.

### Alternatives rejected
| Alternative | Why rejected |
|---|---|
| `aws-sdk-bedrockruntime` | 80-120 crate dependency explosion, hyper conflicts |
| `aws-sign-v4` (third-party) | Sparse maintenance, no session token support |
| `reqsign` | 474K SLoC transitive, wraps `aws-sigv4` anyway |
| Manual SigV4 | Error-prone, security risk |

### Bedrock API shape
- Non-streaming: `POST /model/{modelId}/invoke` with SigV4 auth
- Request body is Anthropic Messages format + `anthropic_version: "bedrock-2023-05-31"`, model in URL not body
- Response body is raw Anthropic JSON (no Bedrock envelope)
- Streaming: `POST /model/{modelId}/invoke-with-response-stream`, returns AWS Event Stream binary framing
- Per-chunk payload is base64-encoded Anthropic SSE JSON after unwrapping binary frame

### Streaming complexity
AWS Event Stream is binary framing (4-byte prelude + headers + payload + CRC32), NOT SSE. Requires either `aws-smithy-eventstream` crate or a manual frame parser (~60-100 lines). After decoding, the content is standard Anthropic streaming events usable by existing `StreamingTranslator`.

### Env vars
`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN` (optional), `AWS_REGION`

### Retryable errors
Existing `is_retryable()` covers 429/5xx/408, which maps correctly to Bedrock's `ThrottlingException`, `ModelTimeoutException`, `ServiceUnavailableException`, `InternalServerException`.

---

## 3. Azure OpenAI Backend (R3)

### Decision
Reuse existing `OpenAIClient` with Azure-specific URL construction and `api-key` auth header. Minimal code changes.

### Rationale
- Azure Chat Completions request/response body is identical to standard OpenAI.
- Streaming SSE format is identical (`data: {...}\n\n` with `data: [DONE]` terminator).
- No changes needed to translator crate, streaming code, or SSE parser.
- The `model` field in JSON body is ignored by Azure (deployment in URL determines model).

### URL format
```
{AZURE_OPENAI_ENDPOINT}/openai/deployments/{AZURE_OPENAI_DEPLOYMENT}/chat/completions?api-version={AZURE_OPENAI_API_VERSION}
```

### Auth
`api-key: {key}` header (not `Authorization: Bearer`). New `BackendAuth::AzureApiKey` variant or reuse existing `RequestAuth::Header { name, value }`.

### Env vars
| Variable | Required | Default |
|---|---|---|
| `AZURE_OPENAI_API_KEY` | Yes | none |
| `AZURE_OPENAI_ENDPOINT` | Yes | none (full URL, e.g., `https://myresource.openai.azure.com`) |
| `AZURE_OPENAI_DEPLOYMENT` | Yes | none |
| `AZURE_OPENAI_API_VERSION` | No | `2024-10-21` |

### What changes in codebase
1. `config/mod.rs`: Add `BackendKind::AzureOpenAI`, parse env vars, construct URL
2. `backend/mod.rs`: Add `BackendClient::AzureOpenAI(OpenAIClient)` variant
3. `backend/openai_client.rs`: Azure arm in URL construction (pre-constructed from config)
4. Auth mapping for `api-key` header

---

## 4. Virtual Key Management (R4)

### Decision
SHA-256 hashed keys in SQLite, `DashMap<[u8;32], VirtualKeyMeta>` as in-memory cache, immediate invalidation on revocation.

### Rationale
- `sha2` and `subtle` already in `Cargo.toml`; existing auth uses SHA-256 + constant-time compare.
- bcrypt/argon2 are wrong for high-entropy API tokens (50-300ms per check at 100 concurrent requests).
- In-memory DashMap avoids SQLite on the hot auth path.
- Follows existing two-phase pattern: SQLite persist, then in-memory apply.

### Schema
```sql
CREATE TABLE IF NOT EXISTS virtual_api_key (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    key_hash       TEXT NOT NULL UNIQUE,
    key_prefix     TEXT NOT NULL,
    description    TEXT,
    created_at     TEXT NOT NULL,
    expires_at     TEXT,
    revoked_at     TEXT,
    spend_limit    REAL,
    rpm_limit      INTEGER,
    tpm_limit      INTEGER,
    total_spend    REAL NOT NULL DEFAULT 0,
    total_requests INTEGER NOT NULL DEFAULT 0,
    total_tokens   INTEGER NOT NULL DEFAULT 0
);
```

### Key generation
Two UUID v4s concatenated (256 bits entropy), prefixed `sk-vk`. Zero new dependencies (`uuid` already in tree).

### Invalidation
- Admin API writes SQLite first, then updates DashMap (insert on create, remove on revoke).
- On startup, load all non-revoked, non-expired keys from SQLite into DashMap.
- Auth check order: env-var keys (existing `ALLOWED_KEY_HASHES`) -> DashMap virtual keys -> reject.

### New dependency
`dashmap` for concurrent HashMap. Only new production dependency required.

---

## 5. Per-Key Rate Limiting (R7)

### Decision
Per-key `Arc<RateLimitState>` stored inside `VirtualKeyMeta` in the DashMap. Sliding window with `Mutex<VecDeque>`.

### Rationale
- `VecDeque` supports O(1) front-drain for expiry.
- Separate locks for RPM and TPM avoids contention.
- In-memory only (no Redis); sufficient for single-process proxy.

### Data structure
```rust
struct RateLimitState {
    rpm_window: Mutex<VecDeque<u64>>,           // timestamps
    tpm_window: Mutex<VecDeque<(u64, u32)>>,    // (timestamp, tokens)
}
```

### Alternatives rejected
Token bucket (worse burst control), Redis (overkill), SQLite (too slow for hot path).

---

## 6. Rust Client Library Improvements (R5)

### Decision
Typed builder pattern, `Stream` return type for SSE, tool-call helpers, comprehensive rustdoc.

### Rationale
- The existing `Client` struct has a simple config struct but no builder ergonomics.
- Streaming returns raw bytes; should return typed `AnthropicStreamEvent`.
- Tool definitions require manual JSON construction; should have typed builders.

### Scope
- `ClientBuilder` with method chaining
- `impl Stream<Item = Result<StreamEvent>>` for streaming responses
- `Tool`, `ToolChoice` builder types
- Re-export all public types from crate root
- Version bump to 0.2.0

---

## 7. OpenTelemetry Export (R6)

### Decision
Feature-gated `otel` in `anyllm_proxy/Cargo.toml`. Use `opentelemetry` 0.31 + `tracing-opentelemetry` 0.32 + `opentelemetry-otlp` 0.31 with `http-proto` + `reqwest-client` transport.

### Rationale
- Reuses existing `reqwest` dependency for OTLP HTTP export; avoids `tonic`/`prost`/`h2` gRPC stack.
- `tracing-opentelemetry` bridges existing `#[tracing::instrument]` spans into OTEL spans without code changes.
- Feature-gated: zero runtime overhead when disabled.

### Version compatibility
`tracing-opentelemetry` 0.32.x requires `opentelemetry` 0.31.x (deliberate +1 offset). Pin all three together.

### Cargo feature config
```toml
[features]
otel = ["opentelemetry", "opentelemetry_sdk", "opentelemetry-otlp", "tracing-opentelemetry"]

[dependencies]
opentelemetry         = { version = "0.31", optional = true }
opentelemetry_sdk     = { version = "0.31", optional = true }
opentelemetry-otlp    = { version = "0.31", features = ["trace", "http-proto", "reqwest-client"], default-features = false, optional = true }
tracing-opentelemetry = { version = "0.32", optional = true }
```

### Initialization
Add `OpenTelemetryLayer` to existing `tracing_subscriber::registry()`. `OtelGuard` struct flushes on shutdown. Must fold into the single `.init()` call in `main.rs`.

### Key env vars
`OTEL_EXPORTER_OTLP_ENDPOINT` (standard), `OTEL_SERVICE_NAME`, `OTEL_TRACES_SAMPLER`.
