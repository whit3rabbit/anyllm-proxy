# HuggingFace

HuggingFace Inference, covering both serverless inference (Inference API) and dedicated Inference Endpoints (TGI/vLLM-backed deployments).

**LiteLLM prefix:** `huggingface/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://huggingface.co/docs/api-inference/en/index

## Authentication

| Variable | Required | Description |
|---|---|---|
| `HUGGINGFACE_API_KEY` | Yes (one of) | HuggingFace user access token |
| `HF_TOKEN` | Yes (one of) | Alias for `HUGGINGFACE_API_KEY`; either is accepted |
| `OPENAI_BASE_URL` | Situational | Required for dedicated Inference Endpoints (see Notes) |

## Quick Start

### Single-Backend (env vars)

Serverless inference (public models on the Inference API):

```bash
BACKEND=huggingface \
  HF_TOKEN=hf_your-token \
  OPENAI_BASE_URL=https://api-inference.huggingface.co/models/meta-llama/Meta-Llama-3.1-8B-Instruct/v1 \
  cargo run -p anyllm_proxy
# Docker:
docker run \
  -e BACKEND=huggingface \
  -e HF_TOKEN=hf_your-token \
  -e OPENAI_BASE_URL=https://api-inference.huggingface.co/models/meta-llama/Meta-Llama-3.1-8B-Instruct/v1 \
  -e PROXY_OPEN_RELAY=true \
  -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

Dedicated Inference Endpoint (per-deployment URL):

```yaml
model_list:
  - model_name: llama-3.1-8b
    litellm_params:
      model: huggingface/meta-llama/Meta-Llama-3.1-8B-Instruct
      api_key: "env:HF_TOKEN"
      api_base: "https://<endpoint-id>.endpoints.huggingface.cloud/v1"
```

Serverless Inference API:

```yaml
model_list:
  - model_name: llama-3.1-8b-serverless
    litellm_params:
      model: huggingface/meta-llama/Meta-Llama-3.1-8B-Instruct
      api_key: "env:HF_TOKEN"
      api_base: "https://api-inference.huggingface.co/models/meta-llama/Meta-Llama-3.1-8B-Instruct/v1"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{"model": "meta-llama/Meta-Llama-3.1-8B-Instruct", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -d '{"model": "meta-llama/Meta-Llama-3.1-8B-Instruct", "messages": [{"role": "user", "content": "Hello"}]}'
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
| `meta-llama/Meta-Llama-3.1-8B-Instruct` | 128k | Llama 3.1 8B, commonly available serverless |
| `meta-llama/Meta-Llama-3.1-70B-Instruct` | 128k | Llama 3.1 70B, requires PRO or dedicated endpoint |
| `mistralai/Mistral-7B-Instruct-v0.3` | 32k | Mistral 7B |

## Notes

HuggingFace has no single shared base URL. There are two deployment types:

- **Serverless Inference API:** `https://api-inference.huggingface.co/models/<org>/<model>/v1`. Available for popular gated and public models; rate-limited on free tier; requires accepting model terms on huggingface.co.
- **Dedicated Inference Endpoints:** `https://<endpoint-id>.endpoints.huggingface.cloud/v1`. Per-deployment URL created in the HuggingFace dashboard. Pay-per-hour pricing with guaranteed capacity.

Always set `OPENAI_BASE_URL` or per-model `api_base` in the YAML config; the default base URL is empty. Tool use support depends on the specific model and TGI version deployed.
