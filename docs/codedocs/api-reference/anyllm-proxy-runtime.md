---
title: "anyllm_proxy Runtime"
description: "Reference for the reusable anyllm_proxy library surface: env parsing, cost helpers, fallback config, and the in-process chat runtime."
---

Source files: `crates/proxy/src/lib.rs`, `crates/proxy/src/env_parser.rs`, `crates/proxy/src/cost/mod.rs`, `crates/proxy/src/fallback/config.rs`, `crates/proxy/src/runtime.rs`, `crates/proxy/src/config/mod.rs`

## Import Paths

```rust
use anyllm_proxy::env_parser::{escape_for_env_file, parse_env_content};
use anyllm_proxy::cost::{ModelPricing, pricing, price_per_million_for_model};
use anyllm_proxy::fallback::config::{parse_fallback_config, load_fallback_config};
use anyllm_proxy::runtime::{ChatCompletionRuntime, ChatCompletionService};
use anyllm_proxy::config::{Config, MultiConfig};
```

## Env Parser

```rust
pub struct ParsedPair {
    pub key: String,
    pub value: String,
    pub line: usize,
}

pub struct EnvWarning {
    pub line: Option<usize>,
    pub key: Option<String>,
    pub message: String,
}

pub struct ParseResult {
    pub pairs: Vec<ParsedPair>,
    pub warnings: Vec<EnvWarning>,
    pub hard_errors: Vec<String>,
}

pub fn escape_for_env_file(s: &str) -> String
pub fn parse_env_content(content: &str) -> ParseResult
```

## Cost Helpers

```rust
pub fn spend_threshold_level(spend: f64, budget: f64) -> u8
pub fn reset_alert_level(key_id: i64)
pub fn pricing() -> &'static ModelPricing
pub fn price_per_million_for_model(model_id: &str) -> Option<(f64, f64)>

pub struct ModelPricingEntry {
    pub model_pattern: String,
    pub input_cost_per_token: f64,
    pub output_cost_per_token: f64,
    pub provider: String,
}

pub struct ModelPricing
pub fn load() -> Self
pub fn load_with_optional_override(path: Option<&str>) -> Self
pub fn price_for_model(&self, model: &str) -> Option<(f64, f64)>
pub fn cost_for_usage(&self, model: &str, input_tokens: u64, output_tokens: u64) -> f64
```

## Fallback Config

```rust
pub struct FallbackConfig {
    pub fallback_chains: HashMap<String, Vec<BackendSpec>>,
}

pub struct BackendSpec {
    pub name: String,
    pub env_prefix: String,
}

pub fn parse_fallback_config(yaml: &str) -> Result<FallbackConfig, serde_yaml::Error>
pub fn load_fallback_config() -> Result<Option<FallbackConfig>, FallbackConfigError>
```

## Chat Runtime

```rust
pub trait ChatCompletionService: Send + Sync {
    fn complete<'a>(
        &'a self,
        req: openai::ChatCompletionRequest,
    ) -> BoxFuture<'a, Result<ChatCompletionResult, ChatCompletionError>>;

    fn complete_stream<'a>(
        &'a self,
        req: openai::ChatCompletionRequest,
    ) -> BoxFuture<'a, Result<ChatCompletionStreamResult, ChatCompletionError>>;
}

pub struct ChatCompletionRuntime
pub fn from_config(config: Config) -> Self
pub fn from_multi_config(config: MultiConfig) -> Self
pub fn from_multi_config_with_model_router(
    config: MultiConfig,
    model_router: Option<Arc<RwLock<ModelRouter>>>,
) -> Self
```

### Runtime result types

```rust
pub struct ChatCompletionResult {
    pub response: openai::ChatCompletionResponse,
    pub usage: Option<openai::ChatUsage>,
    pub rate_limits: RateLimitHeaders,
    pub metadata: ChatCompletionMetadata,
    pub warnings: TranslationWarnings,
}

pub struct ChatCompletionMetadata {
    pub requested_model: String,
    pub selected_backend: String,
    pub mapped_model: String,
    pub backend_kind: BackendKind,
    pub provider_id: Option<String>,
    pub api_format: OpenAIApiFormat,
    pub used_responses_api: bool,
}
```

## Configuration loaders

```rust
pub fn from_env() -> Config
pub fn load() -> LoadResult
pub fn from_single_config(config: &Config) -> MultiConfig
pub fn from_toml_str(toml_str: &str) -> MultiConfig
```

These methods are defined in `crates/proxy/src/config/mod.rs`. They are the main entry points if you want to reuse the proxy's config semantics instead of writing your own loader.

## Example

```rust
use anyllm_proxy::env_parser::parse_env_content;
use anyllm_proxy::cost::ModelPricing;

let parsed = parse_env_content("OPENAI_API_KEY=sk-...
BACKEND=groq");
assert!(parsed.hard_errors.is_empty());

let pricing = ModelPricing::load();
let usd = pricing.cost_for_usage("gpt-4o-mini", 1200, 300);
println!("{usd}");
```
