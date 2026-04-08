# Baseten

ML model deployment platform where each deployed model has a unique endpoint URL.

**LiteLLM prefix:** `baseten/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.baseten.co

## Authentication

| Variable | Required | Description |
|---|---|---|
| `BASETEN_API_KEY` | Yes | API key from app.baseten.co/settings/account/api-keys |

## Quick Start

### Single-Backend (env vars)

Baseten exposes a per-model URL for each deployment. Set `OPENAI_BASE_URL` to your model's endpoint.

```bash
BACKEND=baseten \
  BASETEN_API_KEY=your-key \
  OPENAI_BASE_URL=https://model-<id>.api.baseten.co/environments/production/sync/v1 \
  cargo run -p anyllm_proxy
# Docker:
docker run \
  -e BACKEND=baseten \
  -e BASETEN_API_KEY=your-key \
  -e OPENAI_BASE_URL=https://model-<id>.api.baseten.co/environments/production/sync/v1 \
  -e PROXY_OPEN_RELAY=true \
  -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: my-deployed-model
    litellm_params:
      model: baseten/my-deployed-model
      api_key: "env:BASETEN_API_KEY"
      api_base: "https://model-<id>.api.baseten.co/environments/production/sync/v1"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "my-deployed-model", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "my-deployed-model", "messages": [{"role": "user", "content": "Hello"}]}'
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

## Notable Models

| Model ID | Context | Notes |
|---|---|---|

## Notes

Baseten has no shared model catalog — you deploy your own models and each gets a unique URL. Find your endpoint URL in the Baseten dashboard under the deployment's API tab. Set `OPENAI_BASE_URL` (single-backend) or `api_base` per YAML entry to point at the correct deployment. The model name passed in requests is not validated against Baseten; it is forwarded as-is. Tool use is not supported via the OpenAI-compatible layer.
