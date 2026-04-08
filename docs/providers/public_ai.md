# PublicAI

AI inference platform with an OpenAI-compatible API.

**LiteLLM prefix:** `public_ai/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://publicai.io/docs

## Authentication

| Variable | Required | Description |
|---|---|---|
| `PUBLIC_AI_API_KEY` | Yes | API key from publicai.io |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=public_ai PUBLIC_AI_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=public_ai -e PUBLIC_AI_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: public-ai-model
    litellm_params:
      model: public_ai/<model-id>
      api_key: "env:PUBLIC_AI_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "<model-id>", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "<model-id>", "messages": [{"role": "user", "content": "Hello"}]}'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | — |
| Embeddings | — |
| Vision | — |
| Batch | — |

## Notes

Check the PublicAI console at publicai.io for available model IDs and API key generation. Tool use and embeddings are not supported.
