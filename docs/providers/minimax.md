# MiniMax

MiniMax provides large-context Chinese and multilingual models, including the MiniMax-Text-01 model with a 1M token context window, via an OpenAI-compatible API.

**LiteLLM prefix:** `minimax/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://platform.minimaxi.com/document/introduction

## Authentication

| Variable | Required | Description |
|---|---|---|
| `MINIMAX_API_KEY` | Yes | API key obtained from platform.minimaxi.com |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=minimax MINIMAX_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=minimax -e MINIMAX_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: minimax-text-01
    litellm_params:
      model: minimax/MiniMax-Text-01
      api_key: "env:MINIMAX_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "MiniMax-Text-01", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "MiniMax-Text-01", "messages": [{"role": "user", "content": "Hello"}]}'
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
| `MiniMax-Text-01` | 1M | Flagship model, 1 million token context window |
| `abab6.5s-chat` | 245k | Faster and lower cost than Text-01 |

## Notes

- API endpoint is `https://api.minimax.chat/v1`.
- The platform console (platform.minimaxi.com) is primarily in Chinese.
- MiniMax-Text-01's 1M context window makes it suitable for processing very large documents or codebases in a single request.
