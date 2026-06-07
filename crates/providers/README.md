# anyllm_providers

Provider and model catalog for the [anyllm-proxy](https://github.com/whit3rabbit/llm-translate-api) workspace.

## What this crate is

A pure metadata catalog. No HTTP clients, no async runtime, no I/O. Every `ProviderDef` and `ModelDef` is a `'static` constant, so the entire catalog is built at compile time and lookups are zero-allocation slice scans. The only runtime dependency is `serde`.

The crate answers questions like:

- Does the id `groq` refer to a known provider, and which env var supplies its key?
- Which models does `anthropic` advertise, and what is each one's context window?
- For a given provider id, which backend protocol (OpenAI-compat, Vertex, Bedrock, ...) should the proxy route through?
- Which provider owns the LiteLLM prefix `together_ai/`?

What it deliberately does **not** contain: pricing data (lives in `assets/model_pricing.json` at the workspace root and a packaged proxy copy, embedded into the proxy at compile time and overridable at runtime via `MODEL_PRICING_FILE`).

## Where it fits

Part of the five-crate `anyllm-proxy` workspace:

- `anyllm_translate` - pure format mapping between Anthropic Messages and OpenAI Chat Completions.
- `anyllm_providers` (this crate) - provider and model catalog.
- `anyllm_client` - Anthropic HTTP client.
- `anyllm_batch_engine` - batch job queue and webhook delivery.
- `anyllm_proxy` - axum HTTP server, admin UI, and config parsing. Consumes this crate to validate provider ids in config files, populate the admin UI's provider tab, and resolve LiteLLM-style routing prefixes.

If you only need the catalog (for example, you are building your own proxy, a CLI that lists models, or a config validator), you can depend on this crate directly without pulling in the full proxy.

## What's in the catalog

- **109 LiteLLM-aligned providers** generated from LiteLLM's `model_prices_and_context_window.json`.
- **Legacy compatibility aliases** so old anyllm ids such as `zhipuai`, `gmi_cloud`, `public_ai`, `ai_ml_api`, `github`, `jina`, `exa`, and `stability_ai` still resolve to their LiteLLM-canonical ids.
- **Legacy local-only providers** such as `lm_studio`, `llamafile`, and `hosted_vllm` remain resolvable through lookup functions, but are intentionally omitted from `all_providers()`.
- **Protocol variants**: `OpenAICompat`, `AzureOpenAI`, `VertexAI`, `GeminiOpenAI`, `GeminiNative`, `AnthropicNative`, `BedrockNative`, `Custom`.
- **Three status tiers** so callers can decide how to treat each entry:
  - `Implemented` - HTTP client exists in the proxy and has been live-tested.
  - `Wired` - client exists but not live-tested.
  - `Stub` - metadata only; routed through an existing compatible client at runtime (most OpenAI-compat providers fall here).
- **Five auth kinds**: `Bearer`, `GoogleApiKey`, `AzureApiKey`, `AwsSigV4`, `None`.
- **Provider capabilities**: `chat_completions`, `streaming`, `tool_use`, `embeddings`, `vision`, `batch`.
- **Model capabilities**: `streaming`, `tool_use`, `vision`, `extended_thinking`.

## Using it as a library

In a sibling workspace crate, add a path dep:

```toml
[dependencies]
anyllm_providers = { path = "../providers" }
```

Or from crates.io (published in lockstep with the rest of the workspace):

```toml
[dependencies]
anyllm_providers = "0.9"
```

The four most common calls:

```rust
use anyllm_providers::{
    canonical_provider_id, find_by_litellm_prefix, get_provider, list_models, resolve_backend,
};

// Look up a provider by id.
let groq = get_provider("groq").expect("groq is registered");
println!("{} -> {}", groq.display_name, groq.default_base_url);
println!("API key env vars: {:?}", groq.env_vars);

// List every model registered for a provider.
for model in list_models("anthropic") {
    println!(
        "{} ({} ctx, {} max out)",
        model.id, model.context_window, model.max_output_tokens,
    );
}

// Resolve a provider id to a backend kind string and base URL.
// Returns None for ProviderProtocol::Custom (not yet implemented).
if let Some((kind, url)) = resolve_backend("together_ai") {
    println!("routes through {kind} at {url}");
}

// Map a LiteLLM-style routing prefix back to its provider.
let p = find_by_litellm_prefix("together_ai/").unwrap();
assert_eq!(p.id, "together_ai");

// Canonicalize old local ids before persisting new config.
assert_eq!(canonical_provider_id("zhipuai"), "zai");
```

Other public items (re-exported from the crate root):

- Types: `ProviderDef`, `ProviderProtocol`, `ProviderStatus`, `ProviderCapabilities`, `AuthKind`, `ModelDef`, `ModelStatus`, `ModelCapabilities`.
- Iterators: `all_providers()` returns the generated LiteLLM snapshot.
- Single-model lookup: `get_model(provider_id, model_id)`.
- Migration helper: `canonical_provider_id(provider_id)`.

## Runtime LiteLLM updates

The default crate remains static and does no I/O. If your app needs newer
LiteLLM provider/model rows without waiting for an `anyllm_providers` release,
enable the opt-in runtime catalog features:

```toml
[dependencies]
anyllm_providers = { version = "0.9", features = ["remote-catalog"] }
```

`runtime-catalog` adds owned catalog types and a parser for LiteLLM's
`model_prices_and_context_window.json`. `remote-catalog` also adds explicit
fetch/cache helpers using a caller-provided `reqwest::Client`:

```rust
use anyllm_providers::{ProviderCatalog, RemoteCatalogOptions};

let client = reqwest::Client::builder()
    .redirect(reqwest::redirect::Policy::none())
    .timeout(std::time::Duration::from_secs(30))
    .build()?;

let cache_dir = std::env::temp_dir().join("anyllm-provider-catalog");
let options = RemoteCatalogOptions::default()
    .with_cache_dir(&cache_dir)
    .with_stale_on_error(true);

let catalog = ProviderCatalog::fetch_litellm_with_options(&client, &options).await?;
for p in catalog.all_providers() {
    println!("{}: {} models", p.id, catalog.list_models(&p.id).len());
}
```

Known providers keep the bundled auth, protocol, env-var, and base-url metadata.
Brand-new LiteLLM providers are exposed with `ProviderStatus::Stub`, a guessed
API-key env var, and an empty default base URL so callers do not silently route
to the wrong endpoint.

## Integrating into a TUI (or any client app)

In its default mode this crate gives you the static catalog. It does **not** ship an HTTP client, an async runtime, or a key store, so a TUI integrates it as a read-only data source and supplies its own transport (typically `reqwest`) and config (typically `std::env` or a config file).

The recommended local integration can use **Ollama** and **LM Studio** as built-in providers. Both are local, OpenAI-compatible, require no API key, and their catalog entries (`auth: AuthKind::None`, `protocol: ProviderProtocol::OpenAICompat`) reflect that. Ollama is part of the LiteLLM snapshot; LM Studio is a legacy-only local provider that still resolves through `get_provider("lm_studio")` but is not returned by `all_providers()`.

### Worked example: Ollama + LM Studio

```rust
use anyllm_providers::{get_provider, AuthKind, ProviderDef};

// Two stable ids, hardcoded as the TUI's defaults.
const DEFAULTS: &[&str] = &["ollama", "lm_studio"];

fn default_providers() -> Vec<&'static ProviderDef> {
    DEFAULTS.iter().filter_map(|id| get_provider(id)).collect()
}
```

What you get from the catalog for these two:

| id          | display_name | default_base_url              | auth          |
|-------------|--------------|-------------------------------|---------------|
| `ollama`    | Ollama       | `http://localhost:11434/v1`   | `AuthKind::None` |
| `lm_studio` | LM Studio    | `http://localhost:1234/v1`    | `AuthKind::None` |

Both are `ProviderProtocol::OpenAICompat`, so the same HTTP code talks to either one.

#### Probe whether the local server is running

`env_vars` is empty for self-hosted providers, so the "is this usable?" check is a TCP/HTTP probe instead of an env-var check. The OpenAI-compatible `/models` endpoint is the right thing to hit; it doubles as the model list (see next step):

```rust
async fn is_up(client: &reqwest::Client, p: &ProviderDef) -> bool {
    let url = format!("{}/models", p.default_base_url.trim_end_matches('/'));
    client.get(&url).send().await.map(|r| r.status().is_success()).unwrap_or(false)
}
```

Show a green/red dot next to each entry in the picker based on this. Cache the result for a few seconds so the TUI does not hammer the local server on every redraw.

#### List models from the live server

`list_models("ollama")` and `list_models("lm_studio")` both return an empty slice on purpose, because the actual models are whatever the user has pulled (Ollama) or loaded (LM Studio). Query the live `/v1/models` endpoint at runtime:

```rust
#[derive(serde::Deserialize)]
struct ModelsResp { data: Vec<ModelEntry> }
#[derive(serde::Deserialize)]
struct ModelEntry { id: String }

async fn live_models(client: &reqwest::Client, p: &ProviderDef) -> reqwest::Result<Vec<String>> {
    let url = format!("{}/models", p.default_base_url.trim_end_matches('/'));
    let resp: ModelsResp = client.get(&url).send().await?.json().await?;
    Ok(resp.data.into_iter().map(|m| m.id).collect())
}
```

For Ollama, this returns entries like `llama3.1:8b`, `qwen2.5-coder:7b`. For LM Studio, it returns whatever model is currently loaded (LM Studio serves one at a time by default).

#### Send a chat completion

Same call shape for both providers, since both are OpenAI-compatible and `auth` is `None`:

```rust
async fn chat(
    client: &reqwest::Client,
    p: &ProviderDef,
    model: &str,
    user_msg: &str,
) -> reqwest::Result<String> {
    let url = format!("{}/chat/completions", p.default_base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": user_msg}],
        "stream": false,
    });
    let mut req = client.post(&url).json(&body);
    if !matches!(p.auth, AuthKind::None) {
        // Defensive: catalog says None today, but extending to keyed providers later
        // means this branch will need a key lookup. See "Extending" below.
    }
    let resp: serde_json::Value = req.send().await?.json().await?;
    Ok(resp["choices"][0]["message"]["content"].as_str().unwrap_or_default().to_string())
}
```

That is the entire integration: two ids, one HTTP probe, one model-list call, one chat call. The catalog supplies the URLs, the protocol marker, and the auth kind so none of those strings are hardcoded in the TUI.

### Extending to keyed providers

When you want to add hosted providers (Groq, OpenAI, Anthropic, ...), reuse the same picker and add two pieces:

1. **Read the API key from the canonical env var.** `env_vars` is in priority order; first entry is canonical, the rest are aliases.

   ```rust
   fn api_key_for(p: &ProviderDef) -> Option<String> {
       p.env_vars.iter().find_map(|var| std::env::var(var).ok())
   }
   ```

   Show the canonical name (`p.env_vars[0]`) in any "missing key" hint so the user knows exactly which variable to set.

2. **Branch on `AuthKind` when building the request:**

   ```rust
   let req = match p.auth {
       AuthKind::None         => client.post(&url).json(&body),
       AuthKind::Bearer       => client.post(&url).bearer_auth(api_key).json(&body),
       AuthKind::GoogleApiKey => client.post(&url).header("x-goog-api-key", api_key).json(&body),
       AuthKind::AzureApiKey  => client.post(&url).header("api-key", api_key).json(&body),
       AuthKind::AwsSigV4     => unimplemented!("Bedrock needs SigV4; see crates/proxy/src/backend/bedrock.rs"),
   };
   ```

For keyed providers that ship a static `MODELS` list (OpenAI, Anthropic, Groq, ...), use `list_models(p.id)` instead of probing `/v1/models`, and render capability badges from `ModelCapabilities`:

```rust
for m in anyllm_providers::list_models(p.id) {
    let mut badges = Vec::new();
    if m.capabilities.streaming         { badges.push("stream"); }
    if m.capabilities.tool_use          { badges.push("tools"); }
    if m.capabilities.vision            { badges.push("vision"); }
    if m.capabilities.extended_thinking { badges.push("thinking"); }
    println!("{:30} {:>7} ctx  [{}]", m.id, m.context_window, badges.join(","));
}
```

If `list_models(p.id)` returns empty, treat it as a self-hosted provider and fall back to the live `/v1/models` probe shown above. Local providers that behave like Ollama or LM Studio include `hosted_vllm`, `llamafile`, `xinference`, `docker_model_runner`, and `lemonade`.

### Non-OpenAI-compatible providers

`AnthropicNative`, `GeminiNative`, `BedrockNative`, and `VertexAI` providers use different URL shapes and request bodies. If you do not want to reimplement those, depend on `anyllm_translate` (pure format mapping, no I/O) and/or `anyllm_client` (Anthropic HTTP client) from the same workspace, or run `anyllm_proxy` as a sidecar and point your TUI at it on `http://localhost:3000` instead.

### Persisting selections

`ProviderDef.id` and `ModelDef.id` are stable strings safe to write to a config file. For new configs, persist LiteLLM-canonical provider ids; `canonical_provider_id()` exists only to migrate old local ids. On the next launch, re-resolve them with `get_provider(id)` and `get_model(provider_id, model_id)`. Both return `Option`, so you can detect entries that were removed from a newer version of the catalog and prompt the user to pick again.

## Provider catalog

The advertised catalog is generated from LiteLLM and should not be maintained as a hand-written provider list. Canonical provider ids are the LiteLLM ids, for example `anthropic`, `openai`, `groq`, `together_ai`, `github_copilot`, `gmi`, `publicai`, `aiml`, `jina_ai`, `exa_ai`, `stability`, and `zai`.

To dump the live advertised list yourself:

```rust
for p in anyllm_providers::all_providers() {
    println!("{:24} {:?}  {}", p.id, p.status, p.display_name);
}
```

To check a legacy id while migrating a config:

```rust
assert_eq!(anyllm_providers::canonical_provider_id("gmi_cloud"), "gmi");
assert_eq!(anyllm_providers::canonical_provider_id("public_ai"), "publicai");
assert_eq!(anyllm_providers::canonical_provider_id("zhipuai"), "zai");
```

The non-chat providers (embeddings, STT, TTS, image, search) carry catalog metadata so a UI can list and key-check them, but they are not all wired through the proxy's chat-completions path. Use them via their native APIs or provider-specific passthrough support where available.

## Adding a provider

1. Prefer adding or correcting provider metadata in `scripts/check_litellm_providers.py` source aliases and regenerating `src/providers/litellm_snapshot.rs`.
2. Run `python3 scripts/check_litellm_providers.py --all --write-rust-snapshot crates/providers/src/providers/litellm_snapshot.rs`.
3. Run `python3 scripts/check_litellm_providers.py --all --check`.
4. Only add a hand-written provider module for a local-only or non-LiteLLM provider that must remain resolvable outside the advertised snapshot.

For OpenAI-compatible providers, no proxy-side HTTP code is required; the catalog entry is enough for the proxy to route via its existing `OpenAIClient`.

## Tests

```bash
cargo test -p anyllm_providers
```

The tests in `src/registry.rs` enforce that every non-`Custom` provider resolves to a backend kind and that the LiteLLM prefix lookup is consistent.
