use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "mistral",
    display_name: "Mistral AI",
    default_base_url: "https://api.mistral.ai/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["MISTRAL_API_KEY"],
    litellm_prefix: "mistral/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: true,
        vision: true,
        batch: false,
    },
};

// Models reflect Mistral "La Plateforme" GA offerings as of 2025.
// Context windows come from Mistral's public model pages; max_output values
// mirror the conservative defaults Mistral documents for chat completions.
// Deprecated aliases (e.g. codestral-2405, mistral-tiny, open-mixtral-*) are
// intentionally excluded.
pub const MODELS: &[ModelDef] = &[
    // Frontier generalist
    ModelDef {
        id: "mistral-large-latest",
        provider_id: "mistral",
        context_window: 131_072,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "mistral-medium-latest",
        provider_id: "mistral",
        context_window: 131_072,
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
        id: "mistral-small-latest",
        provider_id: "mistral",
        context_window: 131_072,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Edge / small-footprint
    ModelDef {
        id: "ministral-8b-latest",
        provider_id: "mistral",
        context_window: 131_072,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "ministral-3b-latest",
        provider_id: "mistral",
        context_window: 131_072,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Vision (Pixtral)
    ModelDef {
        id: "pixtral-large-latest",
        provider_id: "mistral",
        context_window: 131_072,
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
        id: "pixtral-12b-2409",
        provider_id: "mistral",
        context_window: 131_072,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Code
    ModelDef {
        id: "codestral-latest",
        provider_id: "mistral",
        context_window: 256_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Regional (Middle East / South Asia focus)
    ModelDef {
        id: "mistral-saba-latest",
        provider_id: "mistral",
        context_window: 32_768,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Reasoning (Magistral)
    ModelDef {
        id: "magistral-medium-latest",
        provider_id: "mistral",
        context_window: 40_000,
        max_output_tokens: 40_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "magistral-small-latest",
        provider_id: "mistral",
        context_window: 40_000,
        max_output_tokens: 40_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // Audio (Voxtral) — accept audio input, produce text
    ModelDef {
        id: "voxtral-small-latest",
        provider_id: "mistral",
        context_window: 32_768,
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
        id: "voxtral-mini-2507",
        provider_id: "mistral",
        context_window: 32_768,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Embeddings
    ModelDef {
        id: "mistral-embed",
        provider_id: "mistral",
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
    // Moderation
    ModelDef {
        id: "mistral-moderation-latest",
        provider_id: "mistral",
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
