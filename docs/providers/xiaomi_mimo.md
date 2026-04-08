# Xiaomi MiMo

Xiaomi's reasoning-focused language model with tool use support.

**LiteLLM prefix:** `xiaomi_mimo/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://api.mimo.chat

## Authentication

| Variable | Required | Description |
|---|---|---|
| `XIAOMI_MIMO_API_KEY` | Yes | API key from api.mimo.chat |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=xiaomi_mimo XIAOMI_MIMO_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=xiaomi_mimo -e XIAOMI_MIMO_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: mimo-7b
    litellm_params:
      model: xiaomi_mimo/MiMo-7B-RL
      api_key: "env:XIAOMI_MIMO_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "MiMo-7B-RL", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "MiMo-7B-RL", "messages": [{"role": "user", "content": "Hello"}]}'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | ✓ |
| Embeddings | — |
| Vision | — |
| Batch | — |

## Notable Models

| Model ID | Notes |
|---|---|
| `MiMo-7B-RL` | 7B reasoning model trained with reinforcement learning |

## Notes

MiMo-7B-RL is a reasoning-specialized model from Xiaomi, trained with reinforcement learning for improved multi-step reasoning. Tool use is supported.
