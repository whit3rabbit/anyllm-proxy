use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Stability AI — primarily image generation; chat completions not supported.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "stability_ai",
    display_name: "Stability AI",
    default_base_url: "https://api.stability.ai/v2beta",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Wired,
    env_vars: &["STABILITY_API_KEY"],
    litellm_prefix: "stability_ai/",
    capabilities: ProviderCapabilities {
        chat_completions: false,
        streaming: false,
        tool_use: false,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "stable-diffusion-3-5-large",
        provider_id: "stability_ai",
        context_window: 0,
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
        id: "stable-diffusion-3-5-large-turbo",
        provider_id: "stability_ai",
        context_window: 0,
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
        id: "stable-diffusion-3-5-medium",
        provider_id: "stability_ai",
        context_window: 0,
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
        id: "stable-diffusion-3-large",
        provider_id: "stability_ai",
        context_window: 0,
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
        id: "stable-diffusion-3-large-turbo",
        provider_id: "stability_ai",
        context_window: 0,
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
        id: "stable-diffusion-3-medium",
        provider_id: "stability_ai",
        context_window: 0,
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
        id: "stable-image-ultra",
        provider_id: "stability_ai",
        context_window: 0,
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
        id: "stable-image-core",
        provider_id: "stability_ai",
        context_window: 0,
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
        id: "stable-diffusion-xl-1024-v1-0",
        provider_id: "stability_ai",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Deprecated,
    },
    ModelDef {
        id: "stable-diffusion-v1-6",
        provider_id: "stability_ai",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Deprecated,
    },
];
