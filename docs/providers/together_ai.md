# Together AI

Large open-source model catalog with serverless and dedicated endpoints.

**LiteLLM prefix:** `together_ai/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.together.ai/docs/openai-api-compatibility

## Authentication

| Variable | Required | Description |
|---|---|---|
| `TOGETHER_API_KEY` | Yes | API key from api.together.xyz (also accepted as `TOGETHERAI_API_KEY`) |
| `TOGETHERAI_API_KEY` | Yes | Alias for `TOGETHER_API_KEY` |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=together_ai TOGETHER_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=together_ai -e TOGETHER_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama-3.2-90b-vision
    litellm_params:
      model: together_ai/meta-llama/Llama-3.2-90B-Vision-Instruct-Turbo
      api_key: "env:TOGETHER_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "meta-llama/Llama-3.2-90B-Vision-Instruct-Turbo", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "meta-llama/Llama-3.2-90B-Vision-Instruct-Turbo", "messages": [{"role": "user", "content": "Hello"}]}'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | ✓ |
| Embeddings | ✓ |
| Vision | ✓ |
| Batch | — |

## Notable Models

| Model ID | Context | Notes |
|---|---|---|
| `meta-llama/Llama-3.2-90B-Vision-Instruct-Turbo` | 131k | Vision-capable Llama 3.2 |
| `mistralai/Mixtral-8x7B-Instruct-v0.1` | 32k | Mixtral MoE |
| `Qwen/Qwen2.5-72B-Instruct-Turbo` | 128k | Qwen 2.5 72B |

## Notes

Together AI offers both serverless pay-per-token endpoints and dedicated GPU deployments. Model IDs follow the `org/model-name` pattern. The full catalog is available at https://api.together.xyz/models. Embedding models are served at the same base URL using the standard `/v1/embeddings` endpoint.
