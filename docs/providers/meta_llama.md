# Meta Llama API

Direct API access to Meta's Llama models, hosted by Meta. Free tier available.

**LiteLLM prefix:** `meta_llama/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://www.llama.com/docs/llama-api

## Authentication

| Variable | Required | Description |
|---|---|---|
| `META_LLAMA_API_KEY` | Yes | API key from llama.com |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=meta_llama META_LLAMA_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=meta_llama -e META_LLAMA_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama-3.3-70b
    litellm_params:
      model: meta_llama/Llama-3.3-70B-Instruct
      api_key: "env:META_LLAMA_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "Llama-3.3-70B-Instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "Llama-3.3-70B-Instruct", "messages": [{"role": "user", "content": "Hello"}]}'
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
| `Llama-3.3-70B-Instruct` | 128k | Llama 3.3 70B instruction-tuned |
| `Llama-3.1-405B-Instruct` | 128k | Llama 3.1 405B, largest public Llama model |

## Notes

The base URL is `https://www.llama.com/api/v1`. Sign up at llama.com for an API key. A free tier with rate-limited access is available. Model IDs do not use an `org/` prefix — pass the model name directly (e.g., `Llama-3.3-70B-Instruct`).
