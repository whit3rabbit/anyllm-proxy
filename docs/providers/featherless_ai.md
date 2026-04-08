# Featherless AI

Featherless AI provides serverless inference for a large catalog of open-weight models from HuggingFace, accessed via an OpenAI-compatible API.

**LiteLLM prefix:** `featherless_ai/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://featherless.ai/docs

## Authentication

| Variable | Required | Description |
|---|---|---|
| `FEATHERLESS_API_KEY` | Yes | API key obtained from featherless.ai |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=featherless_ai FEATHERLESS_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=featherless_ai -e FEATHERLESS_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama-3.1-8b
    litellm_params:
      model: featherless_ai/meta-llama/Meta-Llama-3.1-8B-Instruct
      api_key: "env:FEATHERLESS_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "meta-llama/Meta-Llama-3.1-8B-Instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "meta-llama/Meta-Llama-3.1-8B-Instruct", "messages": [{"role": "user", "content": "Hello"}]}'
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
| `meta-llama/Meta-Llama-3.1-8B-Instruct` | 128k | Llama 3.1 8B instruction-tuned |
| `meta-llama/Meta-Llama-3.1-70B-Instruct` | 128k | Llama 3.1 70B instruction-tuned |
| Any HuggingFace model ID | varies | Full catalog browsable at featherless.ai/models |

## Notes

- API endpoint is `https://api.featherless.ai/v1`.
- Model IDs use the HuggingFace `org/model-name` format exactly as listed on huggingface.co (e.g. `mistralai/Mistral-7B-Instruct-v0.3`).
- Featherless supports serverless (pay-per-token) access to thousands of open-weight models without provisioning dedicated GPU capacity.
- Vision and embeddings are not supported through this provider; use a different provider for those capabilities.
