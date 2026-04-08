# Infinity

Self-hosted embedding server with OpenAI-compatible API, supporting a wide range of sentence-transformer models.

**LiteLLM prefix:** `infinity/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://michaelfeil.eu/infinity/latest/

## Authentication

| Variable | Required | Description |
|---|---|---|
| (none) | — | No authentication required |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=infinity PROXY_OPEN_RELAY=true cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=infinity -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

Override the default endpoint with `OPENAI_BASE_URL` if Infinity is not on localhost.

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: bge-small-en
    litellm_params:
      model: infinity/BAAI/bge-small-en-v1.5
      api_base: "http://localhost:7997"
```

## Usage Examples

### Embeddings (POST /v1/embeddings)

```bash
curl http://localhost:3000/v1/embeddings \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "BAAI/bge-small-en-v1.5", "input": ["Hello world"]}'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | — |
| Streaming | — |
| Tool Use | — |
| Embeddings | ✓ |
| Vision | — |
| Batch | — |

## Notes

Infinity is an embeddings-only server. Start it before running the proxy:

```bash
pip install infinity-emb[all]
infinity_emb v2 --model-id BAAI/bge-small-en-v1.5
```

The server listens on port 7997 by default. Pass any Hugging Face model ID compatible with sentence-transformers. Multiple models can be loaded simultaneously; pass `--model-id` multiple times.
