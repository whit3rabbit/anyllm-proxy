# Quickstart: LiteLLM Parity Features

Developer guide for configuring and testing the new features introduced in `001-litellm-parity`.

## Response Caching

### In-memory (default — no additional setup)

```bash
# Set custom TTL (default 300s) and capacity (default 10,000 entries)
CACHE_TTL_SECS=600 CACHE_MAX_ENTRIES=50000 OPENAI_API_KEY=sk-... cargo run -p anyllm_proxy
```

Verify cache is working:

```bash
# First call — cache miss
curl -s -H "Authorization: Bearer sk-..." \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet-4-6","messages":[{"role":"user","content":"hello"}]}' \
  http://localhost:3000/v1/messages -I | grep x-anyllm-cache
# x-anyllm-cache: miss

# Second identical call — cache hit
curl -s -H "Authorization: Bearer sk-..." \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet-4-6","messages":[{"role":"user","content":"hello"}]}' \
  http://localhost:3000/v1/messages -I | grep x-anyllm-cache
# x-anyllm-cache: hit
```

### Redis cache (optional)

```bash
# Requires redis crate feature
cargo build --features redis

REDIS_URL=redis://localhost:6379 OPENAI_API_KEY=sk-... cargo run -p anyllm_proxy
```

### Disable cache for a specific request

```json
{ "model": "...", "messages": [...], "cache_ttl_secs": 0 }
```

---

## Fallback Chains

Create `proxy_config.yaml`:

```yaml
fallback_chains:
  default:
    - name: azure
      env_prefix: AZURE_FALLBACK_
```

Then set env vars for the fallback backend using the prefix:

```bash
export AZURE_FALLBACK_OPENAI_ENDPOINT=https://myresource.openai.azure.com
export AZURE_FALLBACK_OPENAI_DEPLOYMENT=gpt4o
export AZURE_FALLBACK_OPENAI_API_KEY=your-azure-key

PROXY_CONFIG=./proxy_config.yaml OPENAI_API_KEY=sk-... cargo run -p anyllm_proxy
```

When the primary backend returns 5xx or 429, the proxy silently retries against Azure.

---

## Batch Processing

```bash
# 1. Upload a JSONL file
curl -X POST http://localhost:3000/v1/files \
  -H "Authorization: Bearer sk-..." \
  -F "file=@requests.jsonl" \
  -F "purpose=batch"
# → {"id": "file-abc123", ...}

# 2. Create a batch
curl -X POST http://localhost:3000/v1/batches \
  -H "Authorization: Bearer sk-..." \
  -H "Content-Type: application/json" \
  -d '{"input_file_id":"file-abc123","endpoint":"/v1/chat/completions","completion_window":"24h"}'
# → {"id": "batch-xyz789", "status": "validating", ...}

# 3. Poll status
curl http://localhost:3000/v1/batches/batch-xyz789 \
  -H "Authorization: Bearer sk-..."
# → {"status": "completed", "output_file_id": "file-out-001", ...}
```

Example `requests.jsonl`:
```jsonl
{"custom_id": "req-1", "method": "POST", "url": "/v1/chat/completions", "body": {"model": "gpt-4o", "messages": [{"role": "user", "content": "What is 2+2?"}]}}
{"custom_id": "req-2", "method": "POST", "url": "/v1/chat/completions", "body": {"model": "gpt-4o", "messages": [{"role": "user", "content": "What is the capital of France?"}]}}
```

---

## Cost Tracking

Cost is tracked automatically per virtual key. Query spend:

```bash
# List keys to find ID
curl http://localhost:9000/admin/api/keys \
  -H "Authorization: Bearer sk-admin-..."

# Get spend for key ID 42
curl http://localhost:9000/admin/api/keys/42/spend \
  -H "Authorization: Bearer sk-admin-..."
# → {"key_id": 42, "total_cost_usd": 1.2345, ...}
```

---

## Budget Enforcement

```bash
# Create a key with $0.10 monthly budget
curl -X POST http://localhost:9000/admin/api/keys \
  -H "Authorization: Bearer sk-admin-..." \
  -H "Content-Type: application/json" \
  -d '{"description":"test-key","max_budget_usd":0.10,"budget_duration":"monthly"}'

# Use the key until budget is exhausted — next request returns 429:
# {"error": {"type": "budget_exceeded", "message": "...", ...}}
```

---

## RBAC

```bash
# Create a developer key (default role)
curl -X POST http://localhost:9000/admin/api/keys \
  -H "Authorization: Bearer sk-admin-..." \
  -H "Content-Type: application/json" \
  -d '{"description":"dev-key","role":"developer"}'

# Developer key works on LLM endpoints
curl -X POST http://localhost:3000/v1/messages \
  -H "Authorization: Bearer sk-vk-..." \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet-4-6","messages":[{"role":"user","content":"hi"}]}'
# → 200 OK

# Developer key blocked on admin endpoints
curl http://localhost:9000/admin/api/keys \
  -H "Authorization: Bearer sk-vk-..."
# → 403 Forbidden
```

---

## Audio / Image Passthrough

```bash
# Transcription
curl -X POST http://localhost:3000/v1/audio/transcriptions \
  -H "Authorization: Bearer sk-..." \
  -F "file=@audio.mp3" \
  -F "model=whisper-1"

# Text-to-speech
curl -X POST http://localhost:3000/v1/audio/speech \
  -H "Authorization: Bearer sk-..." \
  -H "Content-Type: application/json" \
  -d '{"model":"tts-1","input":"Hello world","voice":"alloy"}' \
  --output speech.mp3

# Image generation
curl -X POST http://localhost:3000/v1/images/generations \
  -H "Authorization: Bearer sk-..." \
  -H "Content-Type: application/json" \
  -d '{"prompt":"A sunset over mountains","model":"dall-e-3","n":1,"size":"1024x1024"}'
```

---

## Running Tests

```bash
# All tests (unit + integration)
cargo test

# Cache unit tests only
cargo test -p anyllm_proxy cache

# Batch integration tests
cargo test --test batch_api

# Virtual key + budget + RBAC tests
cargo test --test virtual_keys

# With Redis (requires running Redis on localhost:6379)
REDIS_URL=redis://localhost:6379 cargo test --features redis
```
