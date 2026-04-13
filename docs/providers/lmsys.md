# LMSYS (FastChat)

Self-hosted FastChat server for open models (Vicuna, etc.). Point `api_base` at your FastChat instance.

**LiteLLM prefix:** *(not in LiteLLM)*
**Status:** Stub — routes through OpenAI-compatible client
**Default API base:** `http://localhost:8000/v1` (override via managed backend `api_base`)
**Docs:** https://github.com/lm-sys/FastChat

## Authentication

No API key required by default. FastChat servers can optionally require a key.

## Quick Start

```bash
# Start FastChat first, then:
BACKEND=lmsys PROXY_OPEN_RELAY=true cargo run -p anyllm_proxy
```

## Capabilities

| Feature | Supported |
|---|---|
| Chat Completions | ✓ |
| Streaming | ✓ |
| Tool Use | — |
| Embeddings | — |
| Vision | — |
| Batch | — |
