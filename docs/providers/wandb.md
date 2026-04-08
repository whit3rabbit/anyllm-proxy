# Weights & Biases Inference

Hosted model inference integrated with W&B experiment tracking, with per-project endpoint URLs.

**LiteLLM prefix:** `wandb/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.wandb.ai/guides/model-management

## Authentication

| Variable | Required | Description |
|---|---|---|
| `WANDB_API_KEY` | Yes | API key from wandb.ai |

## Quick Start

### Single-Backend (env vars)

W&B Inference does not have a single shared base URL. Each project deployment has its own endpoint. Set `OPENAI_BASE_URL` to your W&B inference endpoint URL.

```bash
BACKEND=wandb \
  WANDB_API_KEY=your-key \
  OPENAI_BASE_URL=https://inference.wandb.ai/<entity>/<project>/v1 \
  cargo run -p anyllm_proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: wandb-model
    litellm_params:
      model: wandb/<model-id>
      api_key: "env:WANDB_API_KEY"
      api_base: "https://inference.wandb.ai/<entity>/<project>/v1"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "<model-id>", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "<model-id>", "messages": [{"role": "user", "content": "Hello"}]}'
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

## Notes

W&B Inference endpoint URLs are specific to each project deployment. Obtain the URL from your W&B project settings and supply it via `api_base` in YAML config or `OPENAI_BASE_URL` env var. Tool use and embeddings are not supported.
