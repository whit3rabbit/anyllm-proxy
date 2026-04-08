# DeepInfra

Serverless GPU inference for open-weight models, with per-token pricing and no minimum commitment.

**LiteLLM prefix:** `deepinfra/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://deepinfra.com/docs

## Authentication

| Variable | Required | Description |
|---|---|---|
| `DEEPINFRA_API_KEY` | Yes | API key from deepinfra.com/dash |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=deepinfra DEEPINFRA_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=deepinfra -e DEEPINFRA_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama-3.1-70b
    litellm_params:
      model: deepinfra/meta-llama/Meta-Llama-3.1-70B-Instruct
      api_key: "env:DEEPINFRA_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "meta-llama/Meta-Llama-3.1-70B-Instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "meta-llama/Meta-Llama-3.1-70B-Instruct", "messages": [{"role": "user", "content": "Hello"}]}'
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
| `meta-llama/Meta-Llama-3.1-70B-Instruct` | 128k | Llama 3.1 70B |
| `Qwen/Qwen2.5-72B-Instruct` | 128k | Qwen 2.5 72B |

## Notes

DeepInfra's base URL is `https://api.deepinfra.com/v1/openai` (includes `/openai` path segment). Model IDs use the HuggingFace `org/model` format. A wide catalog of open-weight models is available; see https://deepinfra.com/models for the full list.
