# AWS Bedrock

AWS Bedrock — Claude, Llama, Titan and other models via AWS managed service.

**LiteLLM prefix:** `bedrock/`  
**Status:** Wired — SigV4 signing implemented, not live-tested  
**Docs:** https://docs.aws.amazon.com/bedrock/latest/APIReference/

## Authentication

| Variable | Required | Description |
|---|---|---|
| `AWS_ACCESS_KEY_ID` | Yes | IAM access key ID |
| `AWS_SECRET_ACCESS_KEY` | Yes | IAM secret access key |
| `AWS_REGION` | Yes | AWS region, e.g. `us-east-1` |
| `AWS_SESSION_TOKEN` | No | Temporary session token (STS/assumed roles) |

## Quick Start

### Single-Backend (env vars)

```bash
BACKEND=bedrock \
  AWS_ACCESS_KEY_ID=AKIA... \
  AWS_SECRET_ACCESS_KEY=... \
  AWS_REGION=us-east-1 \
  cargo run -p anyllm_proxy
# or with Docker:
docker run \
  -e BACKEND=bedrock \
  -e AWS_ACCESS_KEY_ID=AKIA... \
  -e AWS_SECRET_ACCESS_KEY=... \
  -e AWS_REGION=us-east-1 \
  -e PROXY_OPEN_RELAY=true \
  -p 3000:3000 \
  followthewhit3rabbit/anyllm-proxy
```

### LiteLLM YAML Config

```yaml
model_list:
  - model_name: claude-3-5-sonnet
    litellm_params:
      model: bedrock/anthropic.claude-3-5-sonnet-20241022-v2:0
      aws_region_name: us-east-1
  - model_name: llama3-70b
    litellm_params:
      model: bedrock/meta.llama3-70b-instruct-v1:0
      aws_region_name: us-east-1
```

## Usage Examples

### Anthropic Messages API

```bash
curl http://localhost:3000/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $PROXY_API_KEYS" \
  -d '{
    "model": "anthropic.claude-3-5-sonnet-20241022-v2:0",
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
    "model": "anthropic.claude-3-5-sonnet-20241022-v2:0",
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | ✓ |
| Embeddings | — |
| Vision | ✓ |
| Batch | — |

## Notable Models

| Model ID | Context | Max Output | Notes |
|---|---|---|---|
| `anthropic.claude-sonnet-4-20250514-v1:0` | 200k | 16,000 | Claude Sonnet 4, extended thinking |
| `anthropic.claude-haiku-4-5-20251001-v1:0` | 200k | 8,096 | Claude Haiku 4.5, fast |
| `anthropic.claude-3-5-sonnet-20241022-v2:0` | 200k | 8,096 | Claude 3.5 Sonnet v2 |
| `anthropic.claude-3-haiku-20240307-v1:0` | 200k | 4,096 | Claude 3 Haiku, lowest cost |
| `meta.llama3-70b-instruct-v1:0` | 8k | 2,048 | Meta Llama 3 70B |
| `amazon.titan-text-express-v1` | 8k | 8,192 | Amazon Titan text model |

## Notes

- Requests are signed with AWS SigV4. Provide IAM credentials with the `bedrock:InvokeModel` and `bedrock:InvokeModelWithResponseStream` permissions.
- The endpoint is constructed per region: `https://bedrock-runtime.{AWS_REGION}.amazonaws.com`. Defaults to `us-east-1` if `AWS_REGION` is not set.
- Model availability varies by region. Enable the models you need in the AWS Bedrock console before making requests (model access is not automatic).
- This backend is wired (SigV4 signing + Event Stream decoding implemented) but has not been validated against a live AWS endpoint. Report issues if you encounter problems.
- `AWS_SESSION_TOKEN` is required when using temporary credentials from STS `AssumeRole` calls or EC2 instance profiles.
- Amazon Titan embeddings are not currently supported; the embeddings capability is false for this backend.
