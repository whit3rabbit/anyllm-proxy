# AWS SageMaker

AWS SageMaker — managed ML endpoints for custom and third-party models deployed in your AWS account.

> **Status: Not Yet Implemented**
> SageMaker uses SigV4 request signing with a non-standard invocation format. No HTTP client is implemented for this backend. Requests routed to `BACKEND=sagemaker` will not succeed. For production AWS LLM routing today, use the `bedrock` backend instead.

**LiteLLM prefix:** `sagemaker/`  
**Status:** Stub — Custom protocol (SigV4), no HTTP client implemented  
**Docs:** https://docs.aws.amazon.com/sagemaker/latest/APIReference/API_runtime_InvokeEndpoint.html

## Authentication

| Variable | Required | Description |
|---|---|---|
| `AWS_ACCESS_KEY_ID` | Yes | IAM access key ID |
| `AWS_SECRET_ACCESS_KEY` | Yes | IAM secret access key |
| `AWS_REGION_NAME` | Yes | AWS region where the endpoint is deployed, e.g. `us-east-1` |

IAM credentials must have the `sagemaker:InvokeEndpoint` permission on the target endpoint ARN.

## Quick Start

> These examples show the intended configuration once the backend is implemented. They will not work today.

### Single-Backend (env vars)

```bash
BACKEND=sagemaker \
  AWS_ACCESS_KEY_ID=AKIA... \
  AWS_SECRET_ACCESS_KEY=... \
  AWS_REGION_NAME=us-east-1 \
  PROXY_OPEN_RELAY=true \
  cargo run -p anyllm_proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: my-llama-endpoint
    litellm_params:
      model: sagemaker/my-llama3-endpoint
      aws_access_key_id: "env:AWS_ACCESS_KEY_ID"
      aws_secret_access_key: "env:AWS_SECRET_ACCESS_KEY"
      aws_region_name: us-east-1
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "x-api-key: $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "my-llama3-endpoint",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

### OpenAI Chat Completions API

```bash
curl http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer $PROXY_API_KEYS" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "my-llama3-endpoint",
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ (planned) |
| Streaming | ✓ (planned) |
| Tool Use | — |
| Embeddings | ✓ (planned) |
| Vision | — |
| Batch | — |

## Notes

- SageMaker invocation endpoint format: `https://runtime.sagemaker.{region}.amazonaws.com/endpoints/{endpoint-name}/invocations`. The model name in the proxy request maps to the endpoint name.
- SageMaker requires AWS SigV4 request signing, which is distinct from the OpenAI-compatible Bearer token auth used by most other providers. This is why a custom HTTP client is needed and has not yet been implemented.
- The request/response payload format varies by the model container (e.g., TGI, vLLM, Triton). An OpenAI-compatible container (such as a vLLM-based endpoint) would require the least translation work.
- For production AWS LLM routing, use `BACKEND=bedrock` — it has SigV4 signing implemented and supports Claude, Llama, and other managed models without the need to manage your own endpoints.
- `AWS_SESSION_TOKEN` will also be required for temporary credentials once implementation is complete.
