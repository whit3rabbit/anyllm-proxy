# xAI

Grok model series from xAI, accessed via an OpenAI-compatible API.

**LiteLLM prefix:** `xai/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.x.ai/api

## Authentication

| Variable | Required | Description |
|---|---|---|
| `XAI_API_KEY` | Yes | API key obtained from grok.x.ai |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=xai XAI_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=xai -e XAI_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: grok-3
    litellm_params:
      model: xai/grok-3
      api_key: "env:XAI_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "grok-3", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "grok-3", "messages": [{"role": "user", "content": "Hello"}]}'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | ✓ |
| Embeddings | — |
| Vision | ✓ |
| Batch | — |

## Notable Models

| Model ID | Context | Notes |
|---|---|---|
| `grok-3` | 131k | Flagship model |
| `grok-3-fast` | 131k | Lower latency variant |
| `grok-2-1212` | 131k | Previous generation |
| `grok-beta` | 131k | Beta channel |

## Notes

- API keys are issued at grok.x.ai (separate from X/Twitter accounts).
- Base URL is `https://api.x.ai/v1`.
