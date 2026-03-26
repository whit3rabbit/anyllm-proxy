# Contract: POST /v1/chat/completions

Accepts OpenAI Chat Completions format, translates internally through the Anthropic pipeline, returns OpenAI format.

## Request

```
POST /v1/chat/completions
Content-Type: application/json
x-api-key: {key}
```

### Body

```json
{
  "model": "claude-sonnet-4-20250514",
  "messages": [
    {"role": "system", "content": "You are helpful."},
    {"role": "user", "content": "Hello"}
  ],
  "max_tokens": 1024,
  "temperature": 0.7,
  "stream": false
}
```

### Required fields
- `model` (string): Any model ID accepted by the proxy's model mapping
- `messages` (array): At least one message
- `max_tokens` or `max_completion_tokens` (integer): Required (Anthropic constraint); 400 if absent

### Optional fields (translated)
- `temperature`, `top_p`, `stop`, `tools`, `tool_choice`, `user`, `stream`

### Optional fields (dropped with `x-anyllm-degradation`)
- `presence_penalty`, `frequency_penalty`, `response_format`, `logprobs`, `top_logprobs`, `n`, `seed`, `stream_options`

## Response (non-streaming)

```
HTTP/1.1 200 OK
Content-Type: application/json
x-request-id: {uuid}
x-anyllm-degradation: presence_penalty,frequency_penalty
```

```json
{
  "id": "chatcmpl-{uuid}",
  "object": "chat.completion",
  "created": 1711360000,
  "model": "claude-sonnet-4-20250514",
  "choices": [
    {
      "index": 0,
      "message": {
        "role": "assistant",
        "content": "Hello! How can I help?"
      },
      "finish_reason": "stop"
    }
  ],
  "usage": {
    "prompt_tokens": 12,
    "completion_tokens": 8,
    "total_tokens": 20
  }
}
```

## Response (streaming, `stream: true`)

```
HTTP/1.1 200 OK
Content-Type: text/event-stream
Cache-Control: no-cache
```

```
data: {"id":"chatcmpl-...","object":"chat.completion.chunk","created":1711360000,"model":"claude-sonnet-4-20250514","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}

data: {"id":"chatcmpl-...","object":"chat.completion.chunk","created":1711360000,"model":"claude-sonnet-4-20250514","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}

data: {"id":"chatcmpl-...","object":"chat.completion.chunk","created":1711360000,"model":"claude-sonnet-4-20250514","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]
```

## Error responses

All errors returned in OpenAI error format:

```json
{
  "error": {
    "message": "max_tokens is required",
    "type": "invalid_request_error",
    "param": "max_tokens",
    "code": null
  }
}
```

| Condition | Status | Type |
|---|---|---|
| Missing `max_tokens` | 400 | `invalid_request_error` |
| Empty messages | 400 | `invalid_request_error` |
| Invalid API key | 401 | `authentication_error` |
| Rate limited | 429 | `rate_limit_error` |
| Backend error | 502 | `server_error` |
