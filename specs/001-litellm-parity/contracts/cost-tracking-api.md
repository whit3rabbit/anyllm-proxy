# Contract: Cost Tracking API

Per-key spend tracking and reporting via the admin API.

## Admin Endpoints

### GET /admin/api/keys/{id}/spend

Retrieve lifetime and current-period spend for a virtual key.

**Path params**: `id` — virtual key integer ID

**Auth**: Admin key required (403 if developer key).

**Response 200**:
```json
{
  "key_id": 42,
  "key_prefix": "sk-vk-a1b2",
  "total_cost_usd": 1.2345,
  "total_input_tokens": 450000,
  "total_output_tokens": 120000,
  "request_count": 350,
  "period_cost_usd": 0.1234,
  "period_start": "2026-03-01T00:00:00Z",
  "budget_duration": "monthly",
  "max_budget_usd": 50.0
}
```

| Field | Notes |
|-------|-------|
| `total_cost_usd` | Lifetime estimated USD cost |
| `total_input_tokens` | Lifetime input token count |
| `total_output_tokens` | Lifetime output token count |
| `request_count` | Lifetime request count (successful completions only) |
| `period_cost_usd` | Spend in current budget period (0.0 if no `budget_duration` set) |
| `period_start` | Start of current budget period (null if no `budget_duration`) |
| `budget_duration` | `daily`, `monthly`, or null |
| `max_budget_usd` | Budget ceiling (null if no limit) |

**Errors**:
- `404` — key not found
- `403` — caller lacks admin role

---

## Cost Calculation

Estimated cost per request:
```
cost = (input_tokens * input_cost_per_token) + (output_tokens * output_cost_per_token)
```

Pricing is looked up from the bundled `assets/model_pricing.json` by the resolved backend model
name (after model mapping). If no pricing entry matches:
- Cost is recorded as `0.0`
- A `warn!` log line is emitted with the unmatched model name

## Response Headers

### `x-anyllm-cost-usd`

Set on every response where cost was computed (non-zero or explicitly zero with a pricing match).
Value is a string representation of the f64 USD cost, e.g. `"0.002150"`.

Not set if cost tracking is not applicable (e.g., health checks, admin calls).

---

## Streaming Cost Recording

For streaming responses, cost is recorded from the terminal SSE chunk when it includes a `usage`
field. If the backend does not send a terminal usage chunk, cost is recorded as `0.0` for that
request and a `debug!` log is emitted.

The cost update happens asynchronously after the stream closes; it does not block the streaming
response to the client.
