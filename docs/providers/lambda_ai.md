# Lambda AI

GPU cloud provider offering inference for large open-source models at competitive rates.

**LiteLLM prefix:** `lambda_ai/`  
**Status:** Stub — routes through OpenAI-compatible client  
**Docs:** https://lambda.ai/api-documentation

## Authentication

| Variable | Required | Description |
|---|---|---|
| `LAMBDA_API_KEY` | Yes | API key from cloud.lambda.ai |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=lambda_ai LAMBDA_API_KEY=your-key cargo run -p anyllm_proxy
# Docker:
docker run -e BACKEND=lambda_ai -e LAMBDA_API_KEY=your-key -e PROXY_OPEN_RELAY=true -p 3000:3000 followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: llama3.1-405b
    litellm_params:
      model: lambda_ai/llama3.1-405b-instruct-fp8
      api_key: "env:LAMBDA_API_KEY"
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "llama3.1-70b-instruct-fp8", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{"model": "llama3.1-70b-instruct-fp8", "messages": [{"role": "user", "content": "Hello"}]}'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | ✓ |
| Embeddings | — |
| Vision | — |
| Batch | — |

## Notable Models

| Model ID | Context | Notes |
|---|---|---|
| `llama3.1-405b-instruct-fp8` | 128k | Largest Llama 3.1, FP8 quantized |
| `llama3.1-70b-instruct-fp8` | 128k | Balanced quality and speed, FP8 |
| `llama3-8b-instruct` | 8k | Fast, low-cost Llama 3 8B |

## Notes

API keys are managed at cloud.lambda.ai under API Keys. Lambda AI is primarily a GPU cloud platform; the inference API is a separate product. Model availability may vary based on current cluster capacity.
