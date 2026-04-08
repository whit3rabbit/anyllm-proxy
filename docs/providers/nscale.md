# Nscale

EU-based sovereign cloud inference platform for open-source models.

**LiteLLM prefix:** `nscale/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.nscale.com

## Authentication

| Variable | Required | Description |
|---|---|---|
| `NSCALE_API_KEY` | Yes | API key from console.nscale.com |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=nscale NSCALE_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=nscale -e NSCALE_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama-3.3-70b
    litellm_params:
      model: nscale/meta-llama/Llama-3.3-70B-Instruct
      api_key: "env:NSCALE_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "meta-llama/Llama-3.3-70B-Instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "meta-llama/Llama-3.3-70B-Instruct", "messages": [{"role": "user", "content": "Hello"}]}'
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
| `meta-llama/Llama-3.3-70B-Instruct` | 128k | Llama 3.3 70B |
| `Qwen/Qwen2.5-72B-Instruct` | 128k | Qwen 2.5 72B |

## Notes

Nscale infrastructure is located in the EU, making it suitable for workloads with European data residency requirements. Model IDs follow the HuggingFace `org/model-name` convention.
