# Scaleway

Scaleway Generative APIs — European cloud inference with per-deployment endpoint URLs.

**LiteLLM prefix:** `scaleway/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://www.scaleway.com/en/docs/ai-data/generative-apis

## Authentication

| Variable | Required | Description |
|---|---|---|
| `SCW_SECRET_KEY` | Yes | Secret key from console.scaleway.com/iam/api-keys |

## Quick Start

### Single-Backend (env vars)

Scaleway uses per-model endpoint URLs. Set `OPENAI_BASE_URL` to your deployment's endpoint.

```bash
BACKEND=scaleway \
  SCW_SECRET_KEY=your-key \
  OPENAI_BASE_URL=https://api.scaleway.ai/<model-name>/v1 \
  cargo run -p anyllm_proxy
# Docker:
docker run \
  -e BACKEND=scaleway \
  -e SCW_SECRET_KEY=your-key \
  -e OPENAI_BASE_URL=https://api.scaleway.ai/llama-3.3-70b-instruct/v1 \
  -e PROXY_OPEN_RELAY=true \
  -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama-3.3-70b
    litellm_params:
      model: scaleway/llama-3.3-70b-instruct
      api_key: "env:SCW_SECRET_KEY"
      api_base: "https://api.scaleway.ai/llama-3.3-70b-instruct/v1"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "llama-3.3-70b-instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "llama-3.3-70b-instruct", "messages": [{"role": "user", "content": "Hello"}]}'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | ✓ |
| Embeddings | ✓ |
| Vision | — |
| Batch | — |

## Notable Models

| Model ID | Context | Notes |
|---|---|---|
| `llama-3.3-70b-instruct` | 128k | Llama 3.3 70B instruction-tuned |
| `mistral-nemo-instruct-2407` | 128k | Mistral Nemo |

## Notes

There is no single default base URL. Each model deployment has its own endpoint in the form `https://api.scaleway.ai/<model-name>/v1`. You must set either `OPENAI_BASE_URL` (env var, single-backend mode) or `api_base` per entry in a LiteLLM YAML config. API keys are created at console.scaleway.com under IAM > API Keys. Scaleway's infrastructure is EU-hosted, making it suitable for GDPR-sensitive workloads.
