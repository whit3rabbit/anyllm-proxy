# GMI Cloud

Cloud inference platform for open-source LLMs with OpenAI-compatible API.

**LiteLLM prefix:** `gmi_cloud/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://www.gmi.ai/docs

## Authentication

| Variable | Required | Description |
|---|---|---|
| `GMI_CLOUD_API_KEY` | Yes | API key from gmi.ai |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=gmi_cloud GMI_CLOUD_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=gmi_cloud -e GMI_CLOUD_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama-3.3-70b
    litellm_params:
      model: gmi_cloud/meta-llama/Llama-3.3-70B-Instruct
      api_key: "env:GMI_CLOUD_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "meta-llama/Llama-3.3-70B-Instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
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
| `meta-llama/Llama-3.3-70B-Instruct` | 128k | Llama 3.3 70B instruction-tuned |
| `deepseek-ai/DeepSeek-R1` | 64k | DeepSeek R1 reasoning model |

## Notes

Model IDs follow `org/model-name` format. Check https://www.gmi.ai for the current model catalog and pricing.
