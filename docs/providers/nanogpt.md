# NanoGPT

OpenAI-compatible inference API with pay-as-you-go pricing.

**LiteLLM prefix:** `nanogpt/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://nano-gpt.com/docs

## Authentication

| Variable | Required | Description |
|---|---|---|
| `NANOGPT_API_KEY` | Yes | API key from nano-gpt.com |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=nanogpt NANOGPT_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=nanogpt -e NANOGPT_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: nanogpt-model
    litellm_params:
      model: nanogpt/<model-id>
      api_key: "env:NANOGPT_API_KEY"
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

Obtain an API key and browse available models at nano-gpt.com. Tool use and embeddings are not supported.
