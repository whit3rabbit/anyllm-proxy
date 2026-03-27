# Data Model: LiteLLM Parity — Remaining Gap Closure

**Phase 1 output** | Branch: `001-litellm-parity` | Date: 2026-03-26

---

## Entities

### CacheEntry

Stored in-memory (moka) and optionally in Redis. Not persisted to SQLite.

| Field | Type | Notes |
|-------|------|-------|
| `key` | `String` | `{namespace}:{sha256_hex}` e.g. `anth:a3f2...` |
| `response_body` | `Bytes` | Serialized response (JSON for non-streaming; full SSE transcript for streaming cache is out of scope for v1) |
| `created_at` | `Instant` | Set at insert; used by moka for TTL |
| `model` | `String` | Model that produced this response (for logging) |

**Key format**: `{ns}:{hex}` where `ns` is `anth` (Anthropic format) or `oai` (OpenAI format), and
`hex` is the 64-char SHA-256 hex digest of canonical request JSON fields.

**Canonical request fields hashed**: `model`, `messages`, `temperature`, `top_p`, `max_tokens`,
`stop`, `tools`, `tool_choice`. Missing optional fields are excluded from the hash input.

**TTL**: Global default from `CACHE_TTL_SECS` env var (default: 300s). Per-request override via
`cache_ttl_secs` field in the request body (integer, positive, max 86400).

**Streaming**: Caching is only applied to non-streaming requests in v1. Streaming responses bypass
the cache (always `x-anyllm-cache: miss`).

---

### BatchFile

Stored in SQLite `batch_file` table.

| Column | SQL Type | Notes |
|--------|----------|-------|
| `id` | `INTEGER PRIMARY KEY AUTOINCREMENT` | Internal ID |
| `file_id` | `TEXT NOT NULL UNIQUE` | Public ID, `file-{uuid4}` |
| `key_id` | `INTEGER` | FK to `virtual_api_key.id`; NULL for static-key requests |
| `purpose` | `TEXT NOT NULL` | Always `batch` for v1 |
| `filename` | `TEXT` | Original upload filename |
| `byte_size` | `INTEGER NOT NULL` | Raw JSONL byte count |
| `line_count` | `INTEGER NOT NULL` | Number of JSONL lines (validated at upload) |
| `content` | `BLOB NOT NULL` | Raw JSONL bytes (capped at 100MB) |
| `created_at` | `TEXT NOT NULL` | ISO 8601 UTC |

**Validation rules**:
- File must be `multipart/form-data` with `purpose=batch`.
- Content-type must be `application/jsonl` or `text/plain`; reject otherwise with 400.
- Size cap: 100MB (returns 413 if exceeded).
- Each JSONL line must be valid JSON with a `custom_id` string field and a `body` object containing
  a `model` field; invalid lines are rejected at upload time with 400 and a descriptive error
  identifying the first bad line.

---

### BatchJob

Stored in SQLite `batch_job` table.

| Column | SQL Type | Notes |
|--------|----------|-------|
| `id` | `INTEGER PRIMARY KEY AUTOINCREMENT` | Internal ID |
| `batch_id` | `TEXT NOT NULL UNIQUE` | Public ID, `batch-{uuid4}` |
| `key_id` | `INTEGER` | FK to `virtual_api_key.id`; NULL for static-key requests |
| `input_file_id` | `TEXT NOT NULL` | FK to `batch_file.file_id` |
| `backend_batch_id` | `TEXT` | ID returned by the upstream backend (NULL until submitted) |
| `backend_name` | `TEXT NOT NULL` | e.g. `openai`, `azure` |
| `status` | `TEXT NOT NULL` | `validating` / `in_progress` / `completed` / `failed` / `expired` / `cancelling` / `cancelled` |
| `request_counts_total` | `INTEGER NOT NULL DEFAULT 0` | |
| `request_counts_completed` | `INTEGER NOT NULL DEFAULT 0` | |
| `request_counts_failed` | `INTEGER NOT NULL DEFAULT 0` | |
| `output_file_id` | `TEXT` | Populated when status = `completed` |
| `error_file_id` | `TEXT` | Populated when there are per-line errors |
| `created_at` | `TEXT NOT NULL` | ISO 8601 UTC |
| `completed_at` | `TEXT` | ISO 8601 UTC |
| `expires_at` | `TEXT` | ISO 8601 UTC (backend value forwarded) |
| `metadata` | `TEXT` | JSON string; forwarded from create request |

**Status transitions**:
```
validating → in_progress → completed
                        ↘ failed
validating → failed
in_progress → cancelling → cancelled
```

---

### ModelPricingEntry

Loaded from `assets/model_pricing.json` at startup into a `HashMap<String, ModelPricingEntry>`.
Not stored in SQLite.

| Field | Rust Type | Notes |
|-------|-----------|-------|
| `model_pattern` | `String` | Exact model name or prefix for matching |
| `input_cost_per_token` | `f64` | USD per token (input) |
| `output_cost_per_token` | `f64` | USD per token (output) |
| `provider` | `String` | Hint for disambiguation (e.g. `openai`, `anthropic`, `google`) |

**Matching algorithm**: exact match first; then longest prefix match. If no match, cost = 0.0 and
a `warn!` log is emitted.

**JSON format** (`assets/model_pricing.json`):
```json
[
  {
    "model_pattern": "gpt-4o",
    "input_cost_per_token": 0.0000025,
    "output_cost_per_token": 0.00001,
    "provider": "openai"
  },
  ...
]
```

---

### SpendRecord (virtual_api_key table extension)

New columns added to the existing `virtual_api_key` table via a migration:

| Column | SQL Type | Notes |
|--------|----------|-------|
| `role` | `TEXT NOT NULL DEFAULT 'developer'` | `admin` or `developer` |
| `max_budget_usd` | `REAL` | NULL = no limit |
| `budget_duration` | `TEXT` | `daily`, `monthly`, or NULL |
| `period_start` | `TEXT` | ISO 8601 UTC; start of current budget period |
| `period_spend_usd` | `REAL NOT NULL DEFAULT 0.0` | Spend in current budget period |
| `total_input_tokens` | `INTEGER NOT NULL DEFAULT 0` | Lifetime input tokens |
| `total_output_tokens` | `INTEGER NOT NULL DEFAULT 0` | Lifetime output tokens |

**Note**: `total_spend` and `total_requests` and `total_tokens` columns already exist in the
schema; `period_spend_usd` is the new column for budget-period-scoped tracking.

**Budget period reset**: At request time, if `budget_duration` is set and current time > period
boundary, set `period_spend_usd = 0` and `period_start = current_period_start`, then persist
to SQLite (async, non-blocking).

---

### VirtualKeyMeta (DashMap cache extension)

The existing `VirtualKeyMeta` in `admin/keys.rs` gains additional fields:

```rust
pub struct VirtualKeyMeta {
    // existing
    pub id: i64,
    pub rpm_limit: Option<u32>,
    pub tpm_limit: Option<u32>,
    pub spend_limit: Option<f64>,    // existing alias for max_budget_usd
    // new
    pub role: KeyRole,               // Admin | Developer
    pub max_budget_usd: Option<f64>,
    pub budget_duration: Option<BudgetDuration>, // Daily | Monthly
    pub period_start: Option<String>,
    pub period_spend_usd: f64,
}

pub enum KeyRole { Admin, Developer }
pub enum BudgetDuration { Daily, Monthly }
```

---

## SQLite Schema Migrations

The proxy uses `CREATE TABLE IF NOT EXISTS` for idempotent initialization. New tables and column
additions are applied at startup.

**New tables**:
- `batch_file` (see BatchFile above)
- `batch_job` (see BatchJob above)

**Altered table** (`virtual_api_key`): New columns are added via `ALTER TABLE ... ADD COLUMN IF NOT EXISTS`
(supported in SQLite 3.37+, which ships with `rusqlite --features bundled`).

```sql
ALTER TABLE virtual_api_key ADD COLUMN IF NOT EXISTS role TEXT NOT NULL DEFAULT 'developer';
ALTER TABLE virtual_api_key ADD COLUMN IF NOT EXISTS max_budget_usd REAL;
ALTER TABLE virtual_api_key ADD COLUMN IF NOT EXISTS budget_duration TEXT;
ALTER TABLE virtual_api_key ADD COLUMN IF NOT EXISTS period_start TEXT;
ALTER TABLE virtual_api_key ADD COLUMN IF NOT EXISTS period_spend_usd REAL NOT NULL DEFAULT 0.0;
ALTER TABLE virtual_api_key ADD COLUMN IF NOT EXISTS total_input_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE virtual_api_key ADD COLUMN IF NOT EXISTS total_output_tokens INTEGER NOT NULL DEFAULT 0;
```

---

## State Transitions

### Batch Job Status Machine

```
[POST /v1/batches created]
       ↓
  validating
       ↓ (backend accepted)
  in_progress
       ↓ (all lines done)    ↓ (backend failed)
   completed               failed
```

Cancelled states are forwarded from the backend's own cancellation flow.

---

## Environment Variables (new)

| Variable | Default | Description |
|----------|---------|-------------|
| `CACHE_TTL_SECS` | `300` | Default response cache TTL in seconds |
| `CACHE_MAX_ENTRIES` | `10000` | Maximum in-memory cache entries (moka capacity) |
| `REDIS_URL` | (unset) | Redis connection URL; if unset, Redis tier is disabled |
| `PROXY_CONFIG` | (unset) | Path to YAML fallback chain config; if unset, no fallback |
| `QDRANT_URL` | (unset) | Qdrant HTTP URL; required if `--features qdrant` and semantic caching enabled |
| `QDRANT_COLLECTION` | `anyllm_cache` | Qdrant collection name for semantic cache |
| `SEMANTIC_CACHE_THRESHOLD` | `0.95` | Minimum cosine similarity score for a semantic cache hit |
