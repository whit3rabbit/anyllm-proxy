# OVHCloud AI Endpoints

EU-based managed AI inference endpoints with per-deployment URLs.

**LiteLLM prefix:** `ovhcloud/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://endpoints.ai.cloud.ovh.net

## Authentication

| Variable | Required | Description |
|---|---|---|
| `OVH_AI_ENDPOINTS_ACCESS_TOKEN` | Yes | Access token from the OVHCloud AI Endpoints console |

## Quick Start

### Single-Backend (env vars)

OVHCloud does not have a single shared base URL. Each deployment has its own endpoint. Set `OPENAI_BASE_URL` to your deployment URL from the OVHCloud console.

```bash
BACKEND=ovhcloud \
  OVH_AI_ENDPOINTS_ACCESS_TOKEN=your-token \
  OPENAI_BASE_URL=https://<your-endpoint>.endpoints.kepler.ai.cloud.ovh.net/api/openai_compat/v1 \
  cargo run -p anyllm_proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama-70b
    litellm_params:
      model: ovhcloud/Meta-Llama-3.1-70B-Instruct
      api_key: "env:OVH_AI_ENDPOINTS_ACCESS_TOKEN"
      api_base: "https://<your-endpoint>.endpoints.kepler.ai.cloud.ovh.net/api/openai_compat/v1"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "Meta-Llama-3.1-70B-Instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "Meta-Llama-3.1-70B-Instruct", "messages": [{"role": "user", "content": "Hello"}]}'
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
| `Meta-Llama-3.1-70B-Instruct` | Meta Llama 3.1 70B |
| `Mistral-7B-Instruct-v0.3` | Mistral 7B Instruct |

## Notes

Each OVHCloud AI endpoint has a unique URL assigned at deployment time. There is no global base URL. Obtain your endpoint URL from the OVHCloud AI Endpoints console and set it via `api_base` in YAML config or `OPENAI_BASE_URL` env var. Endpoints are hosted in EU data centers.
