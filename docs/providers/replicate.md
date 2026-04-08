# Replicate

Model hosting and inference platform for open-source models via OpenAI-compatible API.

**LiteLLM prefix:** `replicate/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://replicate.com/docs/reference/http

## Authentication

| Variable | Required | Description |
|---|---|---|
| `REPLICATE_API_KEY` | Yes | API token from replicate.com/account/api-tokens |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=replicate REPLICATE_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=replicate -e REPLICATE_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama-3-70b
    litellm_params:
      model: replicate/meta/meta-llama-3-70b-instruct
      api_key: "env:REPLICATE_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "meta/meta-llama-3-70b-instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "meta/meta-llama-3-70b-instruct", "messages": [{"role": "user", "content": "Hello"}]}'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | — |
| Embeddings | — |
| Vision | ✓ |
| Batch | — |

## Notable Models

| Model ID | Context | Notes |
|---|---|---|
| `meta/meta-llama-3-70b-instruct` | 8k | Llama 3 70B instruction-tuned |
| `mistralai/mixtral-8x7b-instruct-v0.1` | 32k | Mixtral MoE instruction-tuned |

## Notes

Replicate exposes an OpenAI-compatible endpoint at `https://openai-compat.replicate.com/v1`. Use this subdomain rather than the standard `api.replicate.com` — the standard API uses a different request/response format. Tool use is not supported via the OpenAI-compatible layer. Models are identified by `owner/model-name` slugs.
