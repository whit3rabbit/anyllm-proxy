# Groq

Ultra-low latency inference powered by LPU hardware. Free tier available.

**LiteLLM prefix:** `groq/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://console.groq.com/docs/openai

## Authentication

| Variable | Required | Description |
|---|---|---|
| `GROQ_API_KEY` | Yes | API key from console.groq.com |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=groq GROQ_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=groq -e GROQ_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama-3.3-70b
    litellm_params:
      model: groq/llama-3.3-70b-versatile
      api_key: "env:GROQ_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "llama-3.3-70b-versatile", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "llama-3.3-70b-versatile", "messages": [{"role": "user", "content": "Hello"}]}'
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
| `llama-3.3-70b-versatile` | 128k | Flagship Llama 3.3 model, best quality |
| `llama-3.1-8b-instant` | 128k | Fast, low-latency |
| `llama3-70b-8192` | 8k | Llama 3 70B |
| `llama3-8b-8192` | 8k | Llama 3 8B |
| `mixtral-8x7b-32768` | 32k | Mixtral MoE |
| `gemma-7b-it` | 8k | Google Gemma 7B |
| `gemma2-9b-it` | 8k | Google Gemma 2 9B |

## Notes

Groq runs inference on proprietary LPU (Language Processing Unit) silicon, delivering significantly lower latency than GPU-based providers. Rate limits on the free tier are enforced per model. Check https://console.groq.com/docs/rate-limits for current limits.
