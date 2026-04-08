# Bytez

Serverless inference for open-weight Hugging Face models, no GPU setup required.

**LiteLLM prefix:** `bytez/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://bytez.com/docs

## Authentication

| Variable | Required | Description |
|---|---|---|
| `BYTEZ_KEY` | Yes | API key from bytez.com |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=bytez BYTEZ_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=bytez -e BYTEZ_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama-3.2-3b
    litellm_params:
      model: bytez/meta-llama/Llama-3.2-3B-Instruct
      api_key: "env:BYTEZ_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "meta-llama/Llama-3.2-3B-Instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "meta-llama/Llama-3.2-3B-Instruct", "messages": [{"role": "user", "content": "Hello"}]}'
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

Bytez provides serverless access to open-weight models published on Hugging Face. Use the Hugging Face model ID (e.g., `meta-llama/Llama-3.2-3B-Instruct`) as the model name in requests. Models cold-start on first request; subsequent requests to the same model are faster. Get an API key at bytez.com. Tool use and embeddings are not supported.
