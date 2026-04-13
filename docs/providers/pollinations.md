# Pollinations

Free AI text and image generation with an OpenAI-compatible endpoint. No API key required for the free tier.

**LiteLLM prefix:** *(not in LiteLLM)*
**Status:** Stub — routes through OpenAI-compatible client
**API base:** `https://text.pollinations.ai/openai`
**Docs:** https://pollinations.ai

## Authentication

No API key required. Set `PROXY_OPEN_RELAY=true` to bypass proxy key enforcement.

## Quick Start

```bash
BACKEND=pollinations PROXY_OPEN_RELAY=true cargo run -p anyllm_proxy
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
