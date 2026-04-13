use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Pollinations — free AI text/image generation with an OpenAI-compatible endpoint.
/// No API key required for the free tier.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "pollinations",
    display_name: "Pollinations",
    default_base_url: "https://text.pollinations.ai/openai",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::None,
    status: ProviderStatus::Stub,
    env_vars: &[],
    litellm_prefix: "",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: false,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

/// Pollinations uses friendly string aliases for models. The actual model routed
/// behind each alias may change over time as Pollinations updates their backend.
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "openai",
        provider_id: "pollinations",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "openai-large",
        provider_id: "pollinations",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "openai-reasoning",
        provider_id: "pollinations",
        context_window: 128_000,
        max_output_tokens: 16_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "mistral",
        provider_id: "pollinations",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "mistral-large",
        provider_id: "pollinations",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "llama",
        provider_id: "pollinations",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "deepseek",
        provider_id: "pollinations",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "deepseek-reasoner",
        provider_id: "pollinations",
        context_window: 128_000,
        max_output_tokens: 16_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "phi",
        provider_id: "pollinations",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "qwen",
        provider_id: "pollinations",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "qwen-coder",
        provider_id: "pollinations",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "sur",
        provider_id: "pollinations",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "sur-mistral",
        provider_id: "pollinations",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "unity",
        provider_id: "pollinations",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
