# Voyage AI

Embeddings-only provider with models optimized for retrieval and semantic search.

**LiteLLM prefix:** `voyage/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://docs.voyageai.com

## Authentication

| Variable | Required | Description |
|---|---|---|
| `VOYAGE_API_KEY` | Yes | API key from dash.voyageai.com |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=voyage VOYAGE_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=voyage -e VOYAGE_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: voyage-3
    litellm_params:
      model: voyage/voyage-3
      api_key: "env:VOYAGE_API_KEY"
```

## Usage Examples

### OpenAI Embeddings API

```bash
curl http://localhost:3000/v1/embeddings \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "voyage-3", "input": "The quick brown fox"}'
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

## Notable Models

| Model ID | Context | Notes |
|---|---|---|
| `voyage-3` | 32k | General-purpose, highest accuracy |
| `voyage-3-lite` | 32k | Faster, lower cost |
| `voyage-code-3` | 32k | Optimized for code retrieval |
| `voyage-multimodal-3` | 32k | Text and image embeddings |

## Notes

Voyage AI is embeddings-only — chat completions are not supported. Use the `/v1/embeddings` endpoint. Do not set this as `BACKEND` for chat workloads. If you need both embeddings and chat in the same config, use a LiteLLM YAML with Voyage for embedding model entries and a separate provider for chat model entries.
