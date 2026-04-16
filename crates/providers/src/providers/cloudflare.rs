use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Cloudflare Workers AI — OpenAI-compatible endpoint is account-scoped.
/// Full base URL pattern: https://api.cloudflare.com/client/v4/accounts/{account_id}/ai/v1
/// The `{account_id}` is resolved at request time from `CLOUDFLARE_ACCOUNT_ID`, so the
/// static `default_base_url` is intentionally left empty; callers must template it.
/// Supported OpenAI-compatible endpoints: `/v1/chat/completions`, `/v1/embeddings`.
/// Auth: `Authorization: Bearer $CLOUDFLARE_API_TOKEN`.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "cloudflare",
    display_name: "Cloudflare Workers AI",
    default_base_url: "",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["CLOUDFLARE_API_KEY", "CLOUDFLARE_ACCOUNT_ID"],
    litellm_prefix: "cloudflare/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: true,
        vision: true,
        batch: false,
    },
};

// GA catalog only. Beta models (e.g. qwq-32b, deepseek-r1-distill) are excluded.
// Context windows reflect what the per-model docs publish; where Cloudflare does
// not publish an explicit `max_tokens` ceiling we set a conservative default.
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "@cf/meta/llama-3.3-70b-instruct-fp8-fast",
        provider_id: "cloudflare",
        context_window: 24_000,
        max_output_tokens: 2_048,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "@cf/meta/llama-3.1-70b-instruct",
        provider_id: "cloudflare",
        context_window: 24_000,
        max_output_tokens: 2_048,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "@cf/meta/llama-3.1-8b-instruct",
        provider_id: "cloudflare",
        context_window: 8_192,
        max_output_tokens: 2_048,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "@cf/meta/llama-3.1-8b-instruct-fast",
        provider_id: "cloudflare",
        context_window: 8_192,
        max_output_tokens: 2_048,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "@cf/meta/llama-3.2-11b-vision-instruct",
        provider_id: "cloudflare",
        context_window: 128_000,
        max_output_tokens: 2_048,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "@cf/meta/llama-3.2-3b-instruct",
        provider_id: "cloudflare",
        context_window: 128_000,
        max_output_tokens: 2_048,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "@cf/meta/llama-3.2-1b-instruct",
        provider_id: "cloudflare",
        context_window: 128_000,
        max_output_tokens: 2_048,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "@cf/mistralai/mistral-small-3.1-24b-instruct",
        provider_id: "cloudflare",
        context_window: 128_000,
        max_output_tokens: 2_048,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "@cf/qwen/qwen2.5-coder-32b-instruct",
        provider_id: "cloudflare",
        context_window: 32_768,
        max_output_tokens: 2_048,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "@cf/google/gemma-3-12b-it",
        provider_id: "cloudflare",
        context_window: 128_000,
        max_output_tokens: 2_048,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Embedding models — chat-completion capabilities set to false.
    ModelDef {
        id: "@cf/baai/bge-large-en-v1.5",
        provider_id: "cloudflare",
        context_window: 512,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "@cf/baai/bge-base-en-v1.5",
        provider_id: "cloudflare",
        context_window: 512,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "@cf/baai/bge-small-en-v1.5",
        provider_id: "cloudflare",
        context_window: 512,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "@cf/baai/bge-m3",
        provider_id: "cloudflare",
        context_window: 8_192,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
