use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Cohere via their OpenAI-compatible compatibility endpoint.
/// Native Cohere v2 chat API (`/v2/chat`) is not implemented; we route through
/// `/compatibility/v1` which accepts OpenAI Chat Completions shape.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "cohere_chat",
    display_name: "Cohere",
    default_base_url: "https://api.cohere.com/compatibility/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["COHERE_API_KEY"],
    litellm_prefix: "cohere_chat/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: true,
        vision: true,
        batch: false,
    },
};

// Only currently GA models per docs.cohere.com/docs/models.
// `command-r-plus`, `command-r`, `command`, `command-light` were deprecated
// 2025-09-15 and are intentionally omitted.
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "command-a-03-2025",
        provider_id: "cohere_chat",
        context_window: 256_000,
        max_output_tokens: 8_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "command-a-reasoning-08-2025",
        provider_id: "cohere_chat",
        context_window: 256_000,
        max_output_tokens: 32_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "command-a-vision-07-2025",
        provider_id: "cohere_chat",
        context_window: 128_000,
        max_output_tokens: 8_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "command-a-translate-08-2025",
        provider_id: "cohere_chat",
        context_window: 8_000,
        max_output_tokens: 8_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "command-r7b-12-2024",
        provider_id: "cohere_chat",
        context_window: 128_000,
        max_output_tokens: 4_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "command-r-plus-08-2024",
        provider_id: "cohere_chat",
        context_window: 128_000,
        max_output_tokens: 4_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "command-r-08-2024",
        provider_id: "cohere_chat",
        context_window: 128_000,
        max_output_tokens: 4_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Aya open-weights models hosted by Cohere.
    ModelDef {
        id: "c4ai-aya-expanse-32b",
        provider_id: "cohere_chat",
        context_window: 128_000,
        max_output_tokens: 4_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "c4ai-aya-vision-32b",
        provider_id: "cohere_chat",
        context_window: 16_000,
        max_output_tokens: 4_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Embedding models. max_output_tokens is not meaningful here; set to 0
    // to match the `text-embedding-3-*` convention in openai.rs.
    ModelDef {
        id: "embed-v4.0",
        provider_id: "cohere_chat",
        context_window: 128_000,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "embed-english-v3.0",
        provider_id: "cohere_chat",
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
        id: "embed-english-light-v3.0",
        provider_id: "cohere_chat",
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
        id: "embed-multilingual-v3.0",
        provider_id: "cohere_chat",
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
        id: "embed-multilingual-light-v3.0",
        provider_id: "cohere_chat",
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
];
