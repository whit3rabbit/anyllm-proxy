# Data Model: LiteLLM Gap Fill

**Date**: 2026-03-25 | **Branch**: `20260325-120000-litellm-gap-fill`

---

## 1. Virtual API Key (SQLite, new table)

### Entity: `virtual_api_key`

| Field | Type | Constraints | Description |
|---|---|---|---|
| `id` | INTEGER | PRIMARY KEY AUTOINCREMENT | Internal row ID |
| `key_hash` | TEXT | NOT NULL, UNIQUE, INDEXED | Hex-encoded SHA-256 of raw key |
| `key_prefix` | TEXT | NOT NULL | First 8 chars of raw key (display only) |
| `description` | TEXT | nullable | Human-readable label |
| `created_at` | TEXT | NOT NULL | ISO 8601 timestamp |
| `expires_at` | TEXT | nullable | ISO 8601 timestamp; NULL = no expiry |
| `revoked_at` | TEXT | nullable | ISO 8601 timestamp; NULL = active |
| `spend_limit` | REAL | nullable | Max USD spend; NULL = unlimited |
| `rpm_limit` | INTEGER | nullable | Max requests/minute; NULL = unlimited |
| `tpm_limit` | INTEGER | nullable | Max tokens/minute; NULL = unlimited |
| `total_spend` | REAL | NOT NULL DEFAULT 0 | Cumulative USD spent |
| `total_requests` | INTEGER | NOT NULL DEFAULT 0 | Cumulative request count |
| `total_tokens` | INTEGER | NOT NULL DEFAULT 0 | Cumulative token count |

### Relationships
- No foreign keys to other tables.
- `key_hash` is the join key between SQLite persistence and in-memory `DashMap`.

### State transitions
- **Created**: `revoked_at = NULL`, `expires_at = NULL or future`
- **Active**: `revoked_at = NULL AND (expires_at IS NULL OR expires_at > now())`
- **Expired**: `expires_at <= now() AND revoked_at IS NULL`
- **Revoked**: `revoked_at IS NOT NULL` (terminal; cannot be un-revoked)

### Validation rules
- `key_hash` must be exactly 64 hex characters (SHA-256).
- `key_prefix` must be 8 characters, starting with `sk-vk`.
- `rpm_limit` and `tpm_limit` must be positive if set.
- `spend_limit` must be non-negative if set.

---

## 2. In-Memory Key Cache (Rust structs)

### `VirtualKeyMeta`

```rust
struct VirtualKeyMeta {
    id: i64,
    description: Option<String>,
    expires_at: Option<u64>,       // epoch seconds
    rpm_limit: Option<u32>,
    tpm_limit: Option<u32>,
    spend_limit: Option<f64>,      // USD
    rate_state: Arc<RateLimitState>,
}
```

### `RateLimitState`

```rust
struct RateLimitState {
    rpm_window: Mutex<VecDeque<u64>>,           // request timestamps (ms)
    tpm_window: Mutex<VecDeque<(u64, u32)>>,    // (timestamp_ms, token_count)
}
```

### Cache structure
`DashMap<[u8; 32], VirtualKeyMeta>` keyed by SHA-256 hash bytes. Stored in `SharedState`.

---

## 3. Backend Configuration (Rust enums, extended)

### `BackendKind` (extended)

Existing variants: `OpenAI`, `OpenAIResponses`, `Vertex`, `GeminiOpenAI`, `Anthropic`

New variants:
- `Bedrock` -- AWS Bedrock with SigV4 auth
- `AzureOpenAI` -- Azure OpenAI with `api-key` header and deployment URL

### `BedrockConfig`

```rust
struct BedrockConfig {
    region: String,                    // AWS_REGION
    access_key_id: String,             // AWS_ACCESS_KEY_ID
    secret_access_key: String,         // AWS_SECRET_ACCESS_KEY (redacted in logs)
    session_token: Option<String>,     // AWS_SESSION_TOKEN
    big_model: String,                 // e.g., "anthropic.claude-3-5-sonnet-20241022-v2:0"
    small_model: String,               // e.g., "anthropic.claude-3-5-haiku-20241022-v1:0"
}
```

### `AzureOpenAIConfig`

```rust
struct AzureOpenAIConfig {
    endpoint: String,                  // AZURE_OPENAI_ENDPOINT (full URL)
    deployment: String,                // AZURE_OPENAI_DEPLOYMENT
    api_key: String,                   // AZURE_OPENAI_API_KEY (redacted in logs)
    api_version: String,               // AZURE_OPENAI_API_VERSION (default: "2024-10-21")
}
```

### `BackendAuth` (extended)

Existing variants: `BearerToken(String)`, `GoogleApiKey(String)`, `None`

New variants:
- `AzureApiKey(String)` -- Maps to `api-key: {value}` header
- `AwsSigV4(BedrockCredentials)` -- SigV4 signing applied per-request

---

## 4. Reverse Translation Types (translator crate, new)

### `ReverseStreamingTranslator`

```rust
struct ReverseStreamingTranslator {
    message_id: String,                // from Anthropic message_start
    model: String,                     // from Anthropic message_start
    tool_call_index: i32,              // tracks current tool_call slot
    input_tokens: Option<u32>,         // from message_start.usage
    output_tokens: Option<u32>,        // from message_delta.usage
}
```

### State transitions
1. `New` -> receives `message_start` -> emits first chunk with `role: "assistant"`
2. `TextContent` -> receives `content_block_delta(TextDelta)` -> emits `delta.content`
3. `ToolContent` -> receives `content_block_start(ToolUse)` -> emits `delta.tool_calls[index]` with id/name
4. `ToolContent` -> receives `content_block_delta(InputJsonDelta)` -> emits `delta.tool_calls[index].function.arguments`
5. `ThinkingContent` -> receives `content_block_delta(ThinkingDelta)` -> emits `delta.reasoning_content`
6. `Done` -> receives `message_delta` -> emits `finish_reason` + optional usage chunk
7. `Done` -> receives `message_stop` -> emits `data: [DONE]`

---

## 5. Client Library Types (anyllm_client, extended)

### `ClientBuilder`

```rust
struct ClientBuilder {
    base_url: Option<String>,
    api_key: Option<String>,
    timeout: Option<Duration>,
    read_timeout: Option<Duration>,
    max_retries: Option<u32>,
    tls_config: Option<TlsConfig>,
}
```

### `ToolBuilder`

```rust
struct ToolBuilder {
    name: String,
    description: Option<String>,
    input_schema: serde_json::Value,
}
```

These are convenience wrappers over existing `Tool` and `ToolChoice` types in the translator crate.
