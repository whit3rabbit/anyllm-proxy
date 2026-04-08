# Gradient AI

Fine-tuning and inference platform for open-weight models.

**LiteLLM prefix:** `gradient_ai/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.gradient.ai

## Authentication

| Variable | Required | Description |
|---|---|---|
| `GRADIENT_ACCESS_TOKEN` | Yes | Access token from gradient.ai |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=gradient_ai GRADIENT_ACCESS_TOKEN=your-token cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=gradient_ai -e GRADIENT_ACCESS_TOKEN=your-token -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama3-gradient
    litellm_params:
      model: gradient_ai/llama3-8b-instruct
      api_key: "env:GRADIENT_ACCESS_TOKEN"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "llama3-8b-instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "llama3-8b-instruct", "messages": [{"role": "user", "content": "Hello"}]}'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | — |
| Embeddings | ✓ |
| Vision | — |
| Batch | — |

## Notes

Gradient AI supports both hosted inference and model fine-tuning. Obtain an access token at gradient.ai. Tool use is not supported. Available models include fine-tuned variants alongside base open-weight models; check the Gradient console for your deployed model IDs.
