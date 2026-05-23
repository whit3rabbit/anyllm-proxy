# Jina AI

Embeddings and reranking provider optimized for search and multimodal retrieval.

**LiteLLM prefix:** `jina_ai/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://jina.ai/embeddings

## Authentication

| Variable | Required | Description |
|---|---|---|
| `JINA_AI_API_KEY` | Yes | API key from jina.ai |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=jina_ai JINA_AI_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=jina_ai -e JINA_AI_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: jina-embeddings-v3
    litellm_params:
      model: jina_ai/jina-embeddings-v3
      api_key: "env:JINA_AI_API_KEY"
```

## Usage Examples

### Embeddings (POST /v1/embeddings)

```bash
curl http://localhost:3000/v1/embeddings \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "jina-embeddings-v3", "input": ["Search query", "Document to embed"]}'
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

| Model ID | Notes |
|---|---|
| `jina-embeddings-v3` | General-purpose text embeddings, multilingual |
| `jina-clip-v2` | Multimodal image and text embeddings |

## Notes

Jina AI is an embeddings-only provider. Chat completions are not supported. Use `POST /v1/embeddings` exclusively. The `jina-clip-v2` model accepts both text and image inputs for cross-modal retrieval.
