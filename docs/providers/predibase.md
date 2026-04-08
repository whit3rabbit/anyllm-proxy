# Predibase

Fine-tuned model serving platform specializing in efficient deployment of LoRA adapters on top of open-source base models.

**LiteLLM prefix:** `predibase/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.predibase.com

## Authentication

| Variable | Required | Description |
|---|---|---|
| `PREDIBASE_API_KEY` | Yes | API key from app.predibase.com |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=predibase PREDIBASE_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=predibase -e PREDIBASE_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama-3-8b
    litellm_params:
      model: predibase/llama-3-1-8b-instruct
      api_key: "env:PREDIBASE_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "llama-3-1-8b-instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "llama-3-1-8b-instruct", "messages": [{"role": "user", "content": "Hello"}]}'
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
| `llama-3-1-8b-instruct` | 128k | Llama 3.1 8B base |
| `mistral-7b-instruct-v0-3` | 32k | Mistral 7B v0.3 base |

## Notes

Predibase's primary use case is serving custom LoRA adapters trained on the platform. To target a fine-tuned adapter, append the adapter name to the model ID using the format `base-model/adapter-name` as documented at https://docs.predibase.com/user-guide/inference/fine-tuned-models. The models listed above are base models available without a custom adapter. Tool use is not supported.
