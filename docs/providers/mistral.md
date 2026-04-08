# Mistral AI

European LLM provider offering instruction-tuned, code, and vision models via an OpenAI-compatible API.

**LiteLLM prefix:** `mistral/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.mistral.ai/api/

## Authentication

| Variable | Required | Description |
|---|---|---|
| `MISTRAL_API_KEY` | Yes | API key from console.mistral.ai |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=mistral MISTRAL_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=mistral -e MISTRAL_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: mistral-large
    litellm_params:
      model: mistral/mistral-large-latest
      api_key: "env:MISTRAL_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "mistral-large-latest", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "mistral-large-latest", "messages": [{"role": "user", "content": "Hello"}]}'
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
| `mistral-large-latest` | 131k | Flagship model, function calling |
| `mistral-small-latest` | 131k | Cost-efficient, function calling |
| `mistral-nemo` | 128k | Jointly developed with NVIDIA |
| `open-mixtral-8x22b` | 65k | Open-weights MoE model |
| `codestral-latest` | 256k | Code generation specialist, no tool use |
| `pixtral-large-latest` | 131k | Vision + text, function calling |

## Notes

`codestral-latest` is served from a separate endpoint (`https://codestral.mistral.ai/v1`). If you need Codestral, use the `codestral` provider instead of `mistral`. Vision capability (`pixtral-large-latest`) is available but not all models support image inputs — check per-model capabilities before use. Embeddings are served at `/v1/embeddings` using the standard OpenAI format.
