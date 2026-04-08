# Cerebras

Ultra-fast inference powered by WSE (Wafer Scale Engine) hardware, delivering some of the lowest latency available for large models.

**LiteLLM prefix:** `cerebras/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://inference-docs.cerebras.ai/introduction

## Authentication

| Variable | Required | Description |
|---|---|---|
| `CEREBRAS_API_KEY` | Yes | API key from inference.cerebras.ai |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=cerebras CEREBRAS_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=cerebras -e CEREBRAS_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama3.3-70b
    litellm_params:
      model: cerebras/llama3.3-70b
      api_key: "env:CEREBRAS_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "llama3.3-70b", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "llama3.3-70b", "messages": [{"role": "user", "content": "Hello"}]}'
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

| Model ID | Context | Notes |
|---|---|---|
| `llama3.3-70b` | 128k | Flagship; best quality on Cerebras hardware |
| `llama3.1-70b` | 8k | Llama 3.1 70B |
| `llama3.1-8b` | 8k | Llama 3.1 8B, lowest latency |

## Notes

Cerebras runs inference on WSE-3 silicon rather than GPU clusters. Token throughput is substantially higher than GPU-based providers, making it suitable for latency-sensitive applications. Context window on the 8B and 70B Llama 3.1 models is capped at 8k by the hardware; the 3.3-70b model supports 128k. Check https://inference-docs.cerebras.ai/rate-limits for current rate limits.
