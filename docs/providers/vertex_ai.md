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
| `VERTEX_API_KEY` | Yes (or alt) | Google API key |
| `GOOGLE_ACCESS_TOKEN` | Yes (or alt) | Short-lived bearer token |

Provide either `VERTEX_API_KEY` or `GOOGLE_ACCESS_TOKEN`. Service-account JSON loading from `GOOGLE_APPLICATION_CREDENTIALS` is not implemented; mint a token externally and pass it via `GOOGLE_ACCESS_TOKEN`.

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=vertex_ai \
  VERTEX_PROJECT=my-project-123 \
  VERTEX_REGION=us-central1 \
  VERTEX_API_KEY=AIza... \
  cargo run -p anyllm_proxy
# or with Docker:
docker run \
  -e BACKEND=vertex_ai \
  -e VERTEX_PROJECT=my-project-123 \
  -e VERTEX_REGION=us-central1 \
  -e VERTEX_API_KEY=AIza... \
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
      api_key: os.environ/VERTEX_API_KEY
      vertex_project: my-project-123
      vertex_location: us-central1
  - model_name: claude-3-5-sonnet-vertex
    litellm_params:
      model: vertex_ai/claude-3-5-sonnet@20241022
      api_key: os.environ/GOOGLE_ACCESS_TOKEN
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

- The OpenAI-compatible base URL is constructed from project and region: `https://{VERTEX_REGION}-aiplatform.googleapis.com/v1/projects/{VERTEX_PROJECT}/locations/{VERTEX_REGION}/endpoints/openapi`.
- Vertex AI serves the same Gemini model IDs as Google AI Studio but requires a GCP project with the Vertex AI API enabled (`gcloud services enable aiplatform.googleapis.com`).
- Claude models (Anthropic Model Garden) use region-specific availability. `us-east5` is the primary region for Claude on Vertex; check the GCP console for current availability.
- If you mint `GOOGLE_ACCESS_TOKEN` from a service account, that account must have the `roles/aiplatform.user` IAM role.
- No static model list is maintained in the proxy. Pass the model ID directly as it appears in the Vertex API.
