---
title: "Embed The Runtime In Rust"
description: "Use anyllm_proxy as a Rust library by embedding the chat-completions runtime instead of launching the full HTTP server."
---

This guide is for Rust applications that want the proxy's backend dispatch and translation behavior without owning the full axum router.

<Steps>
<Step>
### Add the dependency

```toml
[dependencies]
anyllm_proxy = "0.9"
anyllm_translate = "0.9"
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```
</Step>
<Step>
### Build a runtime from env-backed config

```rust
use anyllm_proxy::config::Config;
use anyllm_proxy::runtime::{ChatCompletionRuntime, ChatCompletionService};
use anyllm_translate::openai::ChatCompletionRequest;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env();
    let runtime = ChatCompletionRuntime::from_config(config);

    let request: ChatCompletionRequest = serde_json::from_str(r#"{
      "model": "claude-3-5-sonnet-latest",
      "messages": [{"role": "user", "content": "Say hello"}]
    }"#)?;

    let result = runtime.complete(request).await?;
    println!("{}", result.response.choices[0].message.content.as_ref().unwrap());
    Ok(())
}
```
</Step>
<Step>
### Switch to multi-backend config when needed

```rust
use anyllm_proxy::config::MultiConfig;
use anyllm_proxy::runtime::ChatCompletionRuntime;

let loaded = MultiConfig::load();
let runtime = ChatCompletionRuntime::from_multi_config_with_model_router(
    loaded.multi_config,
    loaded.model_router,
);
```

This reuses the same config loader as the binary, including YAML and TOML detection.
</Step>
</Steps>

## When To Use This Instead Of The Proxy Binary

Embedding the runtime is useful when you already have your own HTTP framework, auth story, or request lifecycle and only want backend selection plus translation. It is not a shortcut to "some of the proxy"; `ChatCompletionRuntime` still assumes the proxy's config and backend abstractions, so it fits best in Rust code that is comfortable depending on the workspace directly.

If you need admin endpoints, request logging, virtual keys, or the Batch API, use the full `anyllm_proxy` server instead. Those features live in `crates/proxy/src/server/`, `crates/proxy/src/admin/`, and `crates/proxy/src/batch/` and are not encapsulated by the runtime alone.

The practical dividing line is ownership of the HTTP boundary. If your application already owns request authentication and wants a library call that accepts `openai::ChatCompletionRequest`, `ChatCompletionRuntime` is the right layer. If you want a complete translation appliance that external tools can call directly, keep the runtime inside the stock proxy binary and let the axum routers own the edge concerns for you.
