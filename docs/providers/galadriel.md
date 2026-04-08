# Galadriel

On-chain verifiable AI inference, providing cryptographic proof of model execution.

**LiteLLM prefix:** `galadriel/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.galadriel.com

## Authentication

| Variable | Required | Description |
|---|---|---|
| `GALADRIEL_API_KEY` | Yes | API key from the Galadriel developer console |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=galadriel GALADRIEL_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=galadriel -e GALADRIEL_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama3.1-70b
    litellm_params:
      model: galadriel/llama3.1-70b
      api_key: "env:GALADRIEL_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "llama3.1-70b", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "llama3.1-70b", "messages": [{"role": "user", "content": "Hello"}]}'
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

| Model ID | Notes |
|---|---|
| `llama3.1-70b` | Meta Llama 3.1 70B, verified inference |
| `llama3.1-8b` | Meta Llama 3.1 8B, verified inference |

## Notes

Galadriel produces verifiable proofs of AI inference, useful for applications that require auditability of model outputs. Tool use and embeddings are not supported.
