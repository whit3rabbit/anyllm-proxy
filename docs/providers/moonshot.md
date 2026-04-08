# Moonshot AI

Moonshot AI (Kimi) provides long-context Chinese and multilingual models via an OpenAI-compatible API.

**LiteLLM prefix:** `moonshot/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://platform.moonshot.cn/docs

## Authentication

| Variable | Required | Description |
|---|---|---|
| `MOONSHOT_API_KEY` | Yes | API key obtained from platform.moonshot.cn |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=moonshot MOONSHOT_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=moonshot -e MOONSHOT_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: moonshot-128k
    litellm_params:
      model: moonshot/moonshot-v1-128k
      api_key: "env:MOONSHOT_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "moonshot-v1-128k", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "moonshot-v1-128k", "messages": [{"role": "user", "content": "Hello"}]}'
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
| `moonshot-v1-8k` | 8k | Lowest cost, short documents |
| `moonshot-v1-32k` | 32k | Mid-range context |
| `moonshot-v1-128k` | 128k | Long documents, full codebases |

## Notes

- API endpoint is `https://api.moonshot.cn/v1`.
- The platform console and documentation are primarily in Chinese; account registration requires a Chinese phone number.
- Model selection determines context window and pricing; use the smallest window that fits your input.
