# Anyscale

Anyscale Endpoints was a managed inference service for open-source models built on Ray. The service has been deprecated.

**LiteLLM prefix:** `anyscale/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.anyscale.com/

## Authentication

| Variable | Required | Description |
|---|---|---|
| `ANYSCALE_API_KEY` | Yes | API key from console.anyscale.com |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=anyscale ANYSCALE_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=anyscale -e ANYSCALE_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama-3-70b
    litellm_params:
      model: anyscale/meta-llama/Llama-3-70b-chat-hf
      api_key: "env:ANYSCALE_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "meta-llama/Llama-3-70b-chat-hf", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "meta-llama/Llama-3-70b-chat-hf", "messages": [{"role": "user", "content": "Hello"}]}'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | — |
| Embeddings | ✓ |
| Vision | — |
| Batch | — |

## Notable Models

| Model ID | Context | Notes |
|---|---|---|
| `meta-llama/Llama-3-70b-chat-hf` | 8k | Llama 3 70B (as of service deprecation) |
| `mistralai/Mixtral-8x7B-Instruct-v0.1` | 32k | Mixtral MoE (as of service deprecation) |

## Notes

**Anyscale Endpoints is deprecated.** New sign-ups are no longer accepted and existing access may be removed. For serverless open-model inference, consider Together AI (`BACKEND=together_ai`), Fireworks AI (`BACKEND=fireworks_ai`), or DeepInfra (`BACKEND=deepinfra`) as drop-in alternatives. The provider stub is retained for compatibility with existing LiteLLM YAML configs that reference the `anyscale/` prefix.
