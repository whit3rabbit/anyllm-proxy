use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "anthropic",
    display_name: "Anthropic",
    default_base_url: "https://api.anthropic.com",
    protocol: ProviderProtocol::AnthropicNative,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Implemented,
    env_vars: &["ANTHROPIC_API_KEY"],
    litellm_prefix: "anthropic/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: false,
        vision: true,
        batch: true,
    },
};

pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "claude-opus-4-6-20260205",
        provider_id: "anthropic",
        context_window: 200_000,
        max_output_tokens: 32_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "claude-sonnet-4-6",
        provider_id: "anthropic",
        context_window: 200_000,
        max_output_tokens: 16_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "claude-opus-4-5-20251101",
        provider_id: "anthropic",
        context_window: 200_000,
        max_output_tokens: 32_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "claude-haiku-4-5-20251001",
        provider_id: "anthropic",
        context_window: 200_000,
        max_output_tokens: 8_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "claude-3-7-sonnet-20250219",
        provider_id: "anthropic",
        context_window: 200_000,
        max_output_tokens: 16_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "claude-3-5-sonnet-20241022",
        provider_id: "anthropic",
        context_window: 200_000,
        max_output_tokens: 8_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "claude-3-5-haiku-20241022",
        provider_id: "anthropic",
        context_window: 200_000,
        max_output_tokens: 8_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "claude-3-opus-20240229",
        provider_id: "anthropic",
        context_window: 200_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "claude-3-haiku-20240307",
        provider_id: "anthropic",
        context_window: 200_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
