# Zhipu AI (Z.AI)

Zhipu AI provides the GLM model series, including vision-capable, reasoning, and free-tier models, via an OpenAI-compatible API. The platform rebranded to Z.AI and updated its endpoint to `api.z.ai`.

**LiteLLM prefix:** `zhipuai/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.z.ai

## Authentication

| Variable | Required | Description |
|---|---|---|
| `ZHIPUAI_API_KEY` | Yes (either) | API key from the Z.AI console |
| `ZAI_API_KEY` | Yes (either) | Alias accepted by the proxy |

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
  - model_name: glm-5.1
    litellm_params:
      model: zhipuai/glm-5.1
      api_key: "env:ZHIPUAI_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "glm-5.1", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "glm-5.1", "messages": [{"role": "user", "content": "Hello"}]}'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | ✓ |
| Embeddings | ✓ |
| Vision | ✓ (glm-5, glm-4.6v, glm-4.5v) |
| Batch | — |
| Thinking / Reasoning | ✓ (glm-5.1, glm-5, glm-4.5) |

## Models

| Model ID | Context | Max Output | Vision | Thinking |
|---|---|---|---|---|
| `glm-5.1` | 200k | 163k | — | ✓ |
| `glm-5` | 128k | 128k | ✓ | ✓ |
| `glm-5-turbo` | 128k | 128k | — | — |
| `glm-4.7` | 128k | 128k | — | — |
| `glm-4.7-flash` | 128k | 128k | — | — |
| `glm-4.6` | 128k | 128k | — | — |
| `glm-4.6v` | 128k | 128k | ✓ | — |
| `glm-4.5` | 128k | 128k | — | ✓ |
| `glm-4.5v` | 128k | 128k | ✓ | — |
| `glm-4.5-air` | 128k | 128k | — | — |
| `glm-4-plus` | 128k | 128k | — | — |
| `glm-4-air` | 128k | 128k | — | — |
| `glm-4-flash` | 128k | 128k | — | — |

`glm-4-flash` is available on a free tier with rate limits — suitable for testing.

## Thinking / Reasoning

When using the Anthropic Messages API with `thinking: {"type": "enabled", "budget_tokens": N}`, the proxy translates this to GLM's native `thinking: {"type": "enabled", "clear_thinking": false}` parameter. Responses include a `reasoning_content` field that the proxy maps back to Anthropic thinking blocks.

When using the OpenAI Chat Completions API, pass the `thinking` parameter directly in the request body.

## Notes

- API endpoint: `https://api.z.ai/api/paas/v4` (previous `open.bigmodel.cn` endpoint still works but is the old branding).
- Rate limiting is concurrency-based (in-flight requests), not request-count based. Error code 1302 indicates rate limit reached.
- The platform console is at https://z.ai; registration is available internationally.
