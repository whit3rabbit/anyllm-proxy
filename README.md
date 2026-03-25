# llm-translate-api (Project Name Pending)

An API translation proxy that allows Anthropic-based applications (like Claude Code, Cursor, Windsurf, or Cline) to interact seamlessly with any OpenAI-compatible backend, local LLMs, or alternative API providers. 

## What, Why, and How?

### What is it?
A lightweight, fast Rust-based proxy that accepts Anthropic Messages API requests, translates them to OpenAI Chat Completions formats, forwards them to any compliant backend, and translates the responses back to the Anthropic format in real-time. It completely supports streaming SSE, tool calling, and image/document blocks.

### Why use it?
- **Local AI Coding:** Run powerful tools like Claude Code against local models (Llama 3, DeepSeek, Qwen) without spending expensive API credits.
- **Broad Compatibility:** Readily works with leading open-weights and alternative models, including Chinese models like Qwen and DeepSeek.
- **Multi-Backend Routing:** Define multiple routes simultaneously. Send simple `haiku` prompts to a fast local model, and complex `opus` requests to external providers, all transparently.
- **Observability via Web UI:** Features a built-in admin dashboard (web interface) allowing you to monitor request logs, latency, error rates, and switch backends dynamically.

### How to Build & Run
First, ensure you have Rust installed.

```bash
# Clone the repository and build
cargo build

# Run the proxy
cargo run -p anthropic_openai_proxy
```
By default, the proxy listens on `0.0.0.0:3000` for incoming API requests and starts a local admin dashboard on `127.0.0.1:3001`.

---

## 1. Primary Use Case: Claude Code + Local LLMs

The easiest way to get started is hooking up a local model (via Ollama, vLLM, or LM Studio) so that Anthropic-centric tools like Claude Code can run entirely on your local machine.

### Example: Running with Ollama (DeepSeek / Qwen)
Ollama exposes an OpenAI-compatible endpoint on port `11434`.

```bash
# 1. Start your local LLM (e.g., DeepSeek Coder or Qwen)
ollama run qwen2.5-coder:32b &

# 2. Start the translation proxy, pointing it to Ollama
# By passing API_KEY=unused, we avoid needing a real OpenAI key.
OPENAI_API_KEY=unused \
OPENAI_BASE_URL=http://localhost:11434/v1 \
BIG_MODEL=qwen2.5-coder:32b \
SMALL_MODEL=qwen2.5-coder:32b \
cargo run -p anthropic_openai_proxy &

# 3. Use Claude Code targeting the local proxy
ANTHROPIC_BASE_URL=http://localhost:3000 claude
```

Use this exact same pattern for **LM Studio** (default port `1234`) or **vLLM** (default port `8000`), simply substituting the `OPENAI_BASE_URL`.

---

## 2. Multi-Routing and the Web Interface

You aren't limited to a single backend. The proxy can read from a TOML configuration file to map different paths or API routes to entirely different LLM providers using different keys.

Create a `config.toml`:

```toml
listen_port = 3000
default_backend = "local_qwen"

[backends.local_qwen]
kind = "openai"
api_key = "unused"
base_url = "http://localhost:11434/v1"
big_model = "qwen2.5-coder:32b"
small_model = "qwen2.5-coder:7b"

[backends.deepseek_api]
kind = "openai"
api_key = "sk-deepseek-..."
base_url = "https://api.deepseek.com/v1"
big_model = "deepseek-coder"
small_model = "deepseek-chat"

[backends.openrouter]
kind = "openai"
api_key = "sk-or-..."
base_url = "https://openrouter.ai/api/v1"
big_model = "anthropic/claude-3.5-sonnet"
small_model = "google/gemini-2.5-flash"
```

Run with this configuration:
```bash
PROXY_CONFIG=config.toml cargo run -p anthropic_openai_proxy
```

This sets up multiple endpoints that your client applications can hit:
- `/v1/messages` ➡️ Routes to `local_qwen` (the default)
- `/deepseek_api/v1/messages` ➡️ Routes to the official DeepSeek API
- `/openrouter/v1/messages` ➡️ Routes to OpenRouter

### The Admin Dashboard
While the proxy is running, open the localhost-only web UI to monitor live traffic, view request histories, or dynamically hot-reload model mappings without restarting the server.

```bash
# Look for "Admin token: <UUID>" in your terminal output
open http://127.0.0.1:3001/admin/?token=YOUR_TOKEN_HERE
```

---

## 3. Commercial APIs (OpenAI, Gemini, OpenRouter)

You can scale up to commercial APIs easily. Here's how to configure the proxy for them if you don't wish to strictly use local setups.

**OpenRouter Example:**
```bash
OPENAI_API_KEY=sk-or-... \
OPENAI_BASE_URL=https://openrouter.ai/api/v1 \
BIG_MODEL=anthropic/claude-3.5-sonnet \
SMALL_MODEL=anthropic/claude-3-haiku \
cargo run -p anthropic_openai_proxy
```

**OpenAI Example:**
```bash
OPENAI_API_KEY=sk-... \
BIG_MODEL=gpt-4o \
SMALL_MODEL=gpt-4o-mini \
cargo run -p anthropic_openai_proxy
```

**Google Gemini Example:**
```bash
BACKEND=gemini \
GEMINI_API_KEY=AIza... \
BIG_MODEL=gemini-2.5-pro \
SMALL_MODEL=gemini-2.5-flash \
cargo run -p anthropic_openai_proxy
```

## Advanced Features

- **Streaming SSE**: Real-time translation of chunked responses, preserving typing feel.
- **Tool Calling**: Transparent tool definition and `tool_use`/`tool_result` translation.
- **Image & Document Blocks**: Parses Base64/URLs and documents seamlessly.
- **Observability**: Built-in SQLite logging for metrics, requests, and latencies.
- **Safety Measures**: SSRF protection, concurrency limits, and exponential backoff retry logic.

## License

MIT
