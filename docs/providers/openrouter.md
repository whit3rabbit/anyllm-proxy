# OpenRouter

Unified gateway to 200+ models from multiple providers via a single API key.

**LiteLLM prefix:** `openrouter/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://openrouter.ai/docs

## Authentication

| Variable | Required | Description |
|---|---|---|
| `OPENROUTER_API_KEY` | Yes | API key from openrouter.ai/keys |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=openrouter OPENROUTER_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=openrouter -e OPENROUTER_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: claude-3.5-sonnet
    litellm_params:
      model: openrouter/anthropic/claude-3.5-sonnet
      api_key: "env:OPENROUTER_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "openai/gpt-4o", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "openai/gpt-4o", "messages": [{"role": "user", "content": "Hello"}]}'
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
| `openai/gpt-4o` | 128k | OpenAI GPT-4o via OpenRouter |
| `anthropic/claude-3.5-sonnet` | 200k | Anthropic Claude 3.5 Sonnet via OpenRouter |
| `meta-llama/llama-3.3-70b-instruct` | 128k | Llama 3.3 70B via OpenRouter |

## Notes

OpenRouter routes requests to the underlying provider transparently. Model IDs use the `provider/model-name` format. For attribution and rate limit tracking, OpenRouter recommends including an `HTTP-Referer` header with your app URL and an `X-Title` header with your app name — these can be added via a LiteLLM YAML `extra_headers` block. Model availability and pricing vary; see https://openrouter.ai/models for the full list. Free tier models are identified with a `:free` suffix (e.g., `meta-llama/llama-3.3-70b-instruct:free`).
