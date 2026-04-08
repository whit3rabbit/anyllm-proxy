# Anthropic

Claude 3.5, Claude 3, and Claude 4 family. Native Anthropic Messages API passthrough.

**LiteLLM prefix:** `anthropic/`  
**Status:** Implemented  
**Docs:** https://docs.anthropic.com/en/api

## Authentication

| Variable | Required | Description |
|---|---|---|
| `ANTHROPIC_API_KEY` | Yes | API key from https://console.anthropic.com/settings/keys |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=anthropic ANTHROPIC_API_KEY=sk-ant-... cargo run -p anyllm_proxy
# or with Docker:
docker run -e BACKEND=anthropic -e ANTHROPIC_API_KEY=sk-ant-... -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: claude-3-5-sonnet
    litellm_params:
      model: anthropic/claude-3-5-sonnet-20241022
      api_key: "env:ANTHROPIC_API_KEY"
  - model_name: claude-3-5-haiku
    litellm_params:
      model: anthropic/claude-3-5-haiku-20241022
      api_key: "env:ANTHROPIC_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{
    "model": "claude-3-5-sonnet-20241022",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{
    "model": "claude-3-5-sonnet-20241022",
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | ✓ |
| Embeddings | — |
| Vision | ✓ |
| Batch | ✓ |

## Notable Models

| Model ID | Context | Max Output | Notes |
|---|---|---|---|
| `claude-opus-4-6-20260205` | 200k | 32,000 | Most capable Claude 4.6, extended thinking |
| `claude-sonnet-4-6` | 200k | 16,000 | Balanced Claude 4.6, extended thinking |
| `claude-opus-4-5-20251101` | 200k | 32,000 | High-capability Claude 4.5, extended thinking |
| `claude-haiku-4-5-20251001` | 200k | 8,096 | Fast Claude 4.5, no extended thinking |
| `claude-3-7-sonnet-20250219` | 200k | 16,000 | Extended thinking support |
| `claude-3-5-sonnet-20241022` | 200k | 8,096 | Previous-generation flagship |
| `claude-3-5-haiku-20241022` | 200k | 8,096 | Fast and affordable |
| `claude-3-opus-20240229` | 200k | 4,096 | Claude 3 flagship |
| `claude-3-haiku-20240307` | 200k | 4,096 | Claude 3 fast/cheap tier |

## Notes

- Requests use the `AnthropicNative` protocol: they are passed through to `https://api.anthropic.com` without translation. The proxy handles auth and routing only.
- Use `/v1/messages` for the Anthropic format. OpenAI Chat Completions requests are translated before forwarding.
- Extended thinking (streaming budget tokens) is supported on `claude-3-7-sonnet-20250219` and all Claude 4.x models. Pass `thinking: {type: "enabled", budget_tokens: N}` in the request body.
- Anthropic does not provide an embeddings API. Route embedding requests to a different backend.
- Batch requests use `/v1/messages/batches` and are forwarded directly to the Anthropic batch endpoint.
