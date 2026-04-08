# Zhipu AI (Z.AI)

Zhipu AI provides the GLM model series, including vision-capable and free-tier models, via an OpenAI-compatible API.

**LiteLLM prefix:** `zhipuai/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://open.bigmodel.cn/dev/api

## Authentication

| Variable | Required | Description |
|---|---|---|
| `ZHIPUAI_API_KEY` | Yes | API key obtained from open.bigmodel.cn |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=zhipuai ZHIPUAI_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=zhipuai -e ZHIPUAI_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: glm-4-plus
    litellm_params:
      model: zhipuai/glm-4-plus
      api_key: "env:ZHIPUAI_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "glm-4-plus", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "glm-4-plus", "messages": [{"role": "user", "content": "Hello"}]}'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | ✓ |
| Embeddings | ✓ |
| Vision | ✓ |
| Batch | — |

## Notable Models

| Model ID | Context | Notes |
|---|---|---|
| `glm-4-plus` | 128k | Flagship GLM-4 model |
| `glm-4-air` | 128k | Balanced cost and capability |
| `glm-4-flash` | 128k | Free tier, rate-limited |
| `glm-4v` | 8k | Vision model, accepts image inputs |

## Notes

- API endpoint is `https://open.bigmodel.cn/api/paas/v4`.
- `glm-4-flash` is available on a free tier with rate limits; suitable for testing and low-volume workloads.
- `glm-4v` supports vision (image inputs); context window is smaller than text-only models.
- The platform console is at open.bigmodel.cn; registration is available internationally.
