# Proxy Architecture

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
