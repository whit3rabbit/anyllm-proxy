---
title: "anyllm_providers"
description: "Reference for the static provider and model catalog crate used by config parsing, routing, and admin discovery."
---

Source files: `crates/providers/src/lib.rs`, `crates/providers/src/provider.rs`, `crates/providers/src/model.rs`, `crates/providers/src/registry.rs`

## Import Path

```rust
use anyllm_providers::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
    ModelCapabilities, ModelDef, ModelStatus,
    canonical_provider_id, get_provider, all_providers, list_models,
    get_model, resolve_backend, find_by_litellm_prefix,
};
```

## Core Types

```rust
pub enum ProviderProtocol {
    OpenAICompat,
    AzureOpenAI,
    VertexAI,
    GeminiOpenAI,
    GeminiNative,
    AnthropicNative,
    BedrockNative,
    Custom,
}

pub enum AuthKind {
    Bearer,
    GoogleApiKey,
    AzureApiKey,
    AwsSigV4,
    None,
}

pub struct ProviderDef {
    pub id: &'static str,
    pub display_name: &'static str,
    pub default_base_url: &'static str,
    pub protocol: ProviderProtocol,
    pub auth: AuthKind,
    pub status: ProviderStatus,
    pub env_vars: &'static [&'static str],
    pub litellm_prefix: &'static str,
    pub capabilities: ProviderCapabilities,
}

pub struct ModelDef {
    pub id: &'static str,
    pub provider_id: &'static str,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub capabilities: ModelCapabilities,
    pub status: ModelStatus,
}
```

## Registry Functions

```rust
pub fn canonical_provider_id(id: &str) -> &str
pub fn get_provider(id: &str) -> Option<&'static ProviderDef>
pub fn all_providers() -> impl Iterator<Item = &'static ProviderDef>
pub fn list_models(provider_id: &str) -> &'static [ModelDef]
pub fn get_model(provider_id: &str, model_id: &str) -> Option<&'static ModelDef>
pub fn resolve_backend(provider_id: &str) -> Option<(&'static str, &'static str)>
pub fn find_by_litellm_prefix(prefix: &str) -> Option<&'static ProviderDef>
```

### Parameters and behavior

| Function | Parameters | Returns | Description |
|---|---|---|---|
| `canonical_provider_id` | `id: &str` | `&str` | Resolves migration aliases such as `zhipuai` -> `zai`. |
| `get_provider` | `id: &str` | `Option<&'static ProviderDef>` | Looks up both canonical and legacy-only providers. |
| `all_providers` | none | iterator | Returns the LiteLLM-aligned catalog, excluding legacy-only local entries. |
| `list_models` | `provider_id: &str` | `&'static [ModelDef]` | Lists static models for a provider id. |
| `resolve_backend` | `provider_id: &str` | `Option<(&'static str, &'static str)>` | Maps provider ids back to proxy backend kinds. |
| `find_by_litellm_prefix` | `prefix: &str` | `Option<&'static ProviderDef>` | Reverses LiteLLM prefixes such as `groq/`. |

## Example

```rust
use anyllm_providers::{get_provider, list_models, resolve_backend};

let groq = get_provider("groq").unwrap();
let models = list_models("groq");
let (kind, url) = resolve_backend("groq").unwrap();

assert_eq!(kind, "openai");
assert!(!models.is_empty());
assert_eq!(groq.id, "groq");
println!("{} -> {}", groq.display_name, url);
```

This crate stays read-only by design. It contains no HTTP clients, pricing logic, or network discovery, which is why the proxy can use it for config validation and admin display without pulling transport concerns into the catalog.
