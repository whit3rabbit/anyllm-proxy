# Google Vertex AI

Google Vertex AI — enterprise Gemini and third-party models via GCP.

**LiteLLM prefix:** `vertex_ai/`  
**Status:** Implemented  
**Docs:** https://cloud.google.com/vertex-ai/generative-ai/docs/reference/rest

## Authentication

| Variable | Required | Description |
|---|---|---|
| `VERTEX_PROJECT` | Yes | GCP project ID (e.g. `my-project-123`) |
| `VERTEX_REGION` | Yes | GCP region (e.g. `us-central1`) |
| `GOOGLE_APPLICATION_CREDENTIALS` | Yes (or alt) | Path to service account JSON key file |
| `VERTEX_API_KEY` | Yes (or alt) | API key if not using a service account |
| `GOOGLE_ACCESS_TOKEN` | No | Short-lived bearer token (overrides key auth) |

Provide either `GOOGLE_APPLICATION_CREDENTIALS` (service account) or `VERTEX_API_KEY`, not both.

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=vertex_ai \
  VERTEX_PROJECT=my-project-123 \
  VERTEX_REGION=us-central1 \
  GOOGLE_APPLICATION_CREDENTIALS=/path/to/sa.json \
  cargo run -p anyllm_proxy
# or with Docker:
docker run \
  -e BACKEND=vertex_ai \
  -e VERTEX_PROJECT=my-project-123 \
  -e VERTEX_REGION=us-central1 \
  -e GOOGLE_APPLICATION_CREDENTIALS=/run/secrets/sa.json \
  -v /path/to/sa.json:/run/secrets/sa.json:ro \
  -e PROXY_OPEN_RELAY=true \
  -p 3000:3000 \
  followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: gemini-2.5-pro
    litellm_params:
      model: vertex_ai/gemini-2.5-pro
      vertex_project: my-project-123
      vertex_location: us-central1
  - model_name: claude-3-5-sonnet-vertex
    litellm_params:
      model: vertex_ai/claude-3-5-sonnet@20241022
      vertex_project: my-project-123
      vertex_location: us-east5
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{
    "model": "gemini-2.5-pro",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{
    "model": "gemini-2.5-pro",
    "messages": [{"role": "user", "content": "Hello"}]
  }'
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
| `gemini-2.5-pro` | 1,048,576 | Flagship Gemini via Vertex, extended thinking |
| `gemini-2.0-flash` | 1,048,576 | Fast multimodal, vision + tools |
| `gemini-1.5-pro` | 2,097,152 | 2M context window |
| `claude-3-5-sonnet@20241022` | 200k | Anthropic Claude via Vertex AI Model Garden |
| `claude-3-haiku@20240307` | 200k | Fast Claude via Vertex AI Model Garden |

## Notes

- The base URL is constructed per request: `https://{VERTEX_REGION}-aiplatform.googleapis.com/v1/projects/{VERTEX_PROJECT}/locations/{VERTEX_REGION}/publishers/google/models/{model}`.
- Vertex AI serves the same Gemini model IDs as Google AI Studio but requires a GCP project with the Vertex AI API enabled (`gcloud services enable aiplatform.googleapis.com`).
- Claude models (Anthropic Model Garden) use region-specific availability. `us-east5` is the primary region for Claude on Vertex; check the GCP console for current availability.
- Service account must have the `roles/aiplatform.user` IAM role.
- No static model list is maintained in the proxy. Pass the model ID directly as it appears in the Vertex API.
