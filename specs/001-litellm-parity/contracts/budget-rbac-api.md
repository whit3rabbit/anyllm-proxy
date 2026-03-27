# Contract: Budget Enforcement and RBAC

Extensions to the virtual key management API.

## Virtual Key Create/Update (extended fields)

### POST /admin/api/keys

**Request body** (additions to existing fields):
```json
{
  "description": "team-alpha dev key",
  "rpm_limit": 100,
  "tpm_limit": 50000,
  "role": "developer",
  "max_budget_usd": 50.0,
  "budget_duration": "monthly"
}
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `role` | string | `"developer"` | `"admin"` or `"developer"` |
| `max_budget_usd` | float | null | Budget ceiling in USD; null means no limit |
| `budget_duration` | string | null | `"daily"` or `"monthly"`; null means no automatic reset |

**Response 200**: Same as existing key create response, plus new fields:
```json
{
  "id": 42,
  "key": "sk-vk-...",
  "key_prefix": "sk-vk-a1b2",
  "role": "developer",
  "max_budget_usd": 50.0,
  "budget_duration": "monthly",
  "created_at": "2026-03-26T00:00:00Z"
}
```

---

## RBAC Rules

| Key type | LLM endpoints | Admin endpoints |
|----------|--------------|-----------------|
| Static env key (`PROXY_API_KEYS`) | Allowed | Allowed |
| Virtual key, `role: admin` | Allowed | Allowed |
| Virtual key, `role: developer` | Allowed | **403 Forbidden** |

**LLM endpoints** (developer role permitted):
- `POST /v1/messages`
- `POST /v1/chat/completions`
- `POST /v1/embeddings`
- `POST /v1/audio/transcriptions`
- `POST /v1/audio/speech`
- `POST /v1/images/generations`
- `POST /v1/files`
- `POST /v1/batches`
- `GET /v1/batches/{id}`
- `GET /v1/batches`

**Admin endpoints** (developer role blocked):
- All paths starting with `/admin/`

**403 response body**:
```json
{
  "error": {
    "type": "permission_denied",
    "message": "This key does not have permission to access admin endpoints."
  }
}
```

---

## Budget Enforcement

### 429 Budget Exceeded

When `max_budget_usd` is set and `period_spend_usd >= max_budget_usd`:

**Response 429**:
```json
{
  "error": {
    "type": "budget_exceeded",
    "message": "This API key has exhausted its budget. Current period spend: $50.12 of $50.00 limit.",
    "budget_limit_usd": 50.0,
    "period_spend_usd": 50.12,
    "budget_duration": "monthly",
    "period_reset_at": "2026-04-01T00:00:00Z"
  }
}
```

**Distinguishing from rate limit 429**: The `error.type` field is `budget_exceeded` vs
`rate_limit_exceeded` for RPM/TPM violations.

### Budget Check Order in Middleware

1. Auth validation (key lookup)
2. RPM/TPM pre-check (existing)
3. **Budget pre-check (new)**: reject if `period_spend_usd >= max_budget_usd`
4. Forward to backend

### Period Reset

On first request after period boundary:
1. Compute new period start (UTC midnight for daily; first of month UTC midnight for monthly)
2. Reset `period_spend_usd = 0.0` and `period_start = new_period_start` in DashMap and SQLite
3. Proceed with the request (the reset counts as the start of the new period, not a rejection)

---

## GET /admin/api/keys (list) — extended response

Each key in the list response now includes `role`, `max_budget_usd`, `budget_duration`, and
`period_spend_usd` fields alongside the existing fields.
