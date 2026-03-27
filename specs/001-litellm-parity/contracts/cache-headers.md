# Contract: Response Cache Headers

All requests to `/v1/messages` and `/v1/chat/completions` include cache status headers.

## Headers

### `x-anyllm-cache`

| Value | When |
|-------|------|
| `miss` | No cache entry found; upstream call made and response (if successful) was cached |
| `hit` | Exact-match cache entry found; upstream call skipped |
| `semantic-hit` | Semantic cache entry found (requires `--features qdrant` + `QDRANT_URL`); upstream call skipped |
| `bypass` | Request was not eligible for caching (streaming, or `cache_ttl_secs: 0` in body) |

**Always present**: The header is set on every response, including errors. Error responses are not
cached and always return `miss` or `bypass`.

## Request Body Extension

The following optional field is accepted in `POST /v1/messages` and `POST /v1/chat/completions`
bodies:

```json
{
  "cache_ttl_secs": 600
}
```

| Value | Behavior |
|-------|----------|
| Absent or null | Use global `CACHE_TTL_SECS` default |
| `0` | Disable caching for this request; header will be `bypass` |
| Positive integer (max 86400) | Override TTL for this request |
| Negative or >86400 | 400 Bad Request |

## Cache Eligibility Rules

A request is cache-eligible if ALL of the following are true:
1. Method is POST; path is `/v1/messages` or `/v1/chat/completions`
2. Request is non-streaming (`"stream": false` or absent)
3. `cache_ttl_secs` is not `0`
4. At least one of `messages` array is non-empty

Ineligible requests always return `x-anyllm-cache: bypass`.

## Cache Key

SHA-256 of canonical JSON of: `{model, messages, temperature, top_p, max_tokens, stop, tools, tool_choice}`.
Missing optional fields are excluded. Fields are sorted alphabetically before hashing.
Namespace prefix: `anth:` for `/v1/messages`, `oai:` for `/v1/chat/completions`.

## Error Behavior

If the Redis cache backend is unreachable, the proxy logs the error, falls back to in-memory cache
only, and continues serving. Cache failures never block the request path.
