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

// Model catalog mirrors docs.anthropic.com model overview + deprecations page.
// Latest GA: Opus 4.6, Sonnet 4.6 (1M context), Haiku 4.5 (200k context).
// Legacy-but-active: Opus 4.5/4.1, Sonnet 4.5. Deprecated-not-retired: Opus 4 / Sonnet 4 /
// Haiku 3 (retire mid-2026). Retired models (3.7 Sonnet, 3.5 Sonnet/Haiku, 3 Opus) are omitted.
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "claude-opus-4-6",
        provider_id: "anthropic",
        context_window: 1_000_000,
        max_output_tokens: 128_000,
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
        context_window: 1_000_000,
        max_output_tokens: 64_000,
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
        max_output_tokens: 64_000,
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
        max_output_tokens: 64_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "claude-sonnet-4-5-20250929",
        provider_id: "anthropic",
        context_window: 200_000,
        max_output_tokens: 64_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "claude-opus-4-1-20250805",
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
    // Deprecated: retires June 15, 2026. Migrate to claude-sonnet-4-6.
    ModelDef {
        id: "claude-sonnet-4-20250514",
        provider_id: "anthropic",
        context_window: 200_000,
        max_output_tokens: 64_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Deprecated,
    },
    // Deprecated: retires June 15, 2026. Migrate to claude-opus-4-6.
    ModelDef {
        id: "claude-opus-4-20250514",
        provider_id: "anthropic",
        context_window: 200_000,
        max_output_tokens: 32_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Deprecated,
    },
    // Deprecated: retires April 20, 2026. Migrate to claude-haiku-4-5-20251001.
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
        status: ModelStatus::Deprecated,
    },
];
