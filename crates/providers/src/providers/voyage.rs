use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Voyage AI — embeddings and reranker provider.
///
/// Voyage exposes `/v1/embeddings` and `/v1/rerank` endpoints. It does not
/// offer chat completions, streaming, or tool use. Authentication is a
/// bearer token (`Authorization: Bearer $VOYAGE_API_KEY`).
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "voyage",
    display_name: "Voyage AI",
    default_base_url: "https://api.voyageai.com/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["VOYAGE_API_KEY"],
    litellm_prefix: "voyage/",
    capabilities: ProviderCapabilities {
        chat_completions: false,
        streaming: false,
        tool_use: false,
        embeddings: true,
        // voyage-multimodal-3 accepts interleaved text+image inputs via the
        // multimodal embeddings endpoint; flagging vision at the provider
        // level since at least one GA model supports it.
        vision: true,
        batch: false,
    },
};

// Context windows verified against https://docs.voyageai.com/docs/embeddings
// (retrieved April 2026). `max_output_tokens = 0` for all embedding models —
// they return vectors, not generated tokens.
pub const MODELS: &[ModelDef] = &[
    // General-purpose (Voyage 3.x generation, GA).
    ModelDef {
        id: "voyage-3-large",
        provider_id: "voyage",
        context_window: 32_000,
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
        id: "voyage-3.5",
        provider_id: "voyage",
        context_window: 32_000,
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
        id: "voyage-3.5-lite",
        provider_id: "voyage",
        context_window: 32_000,
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
        id: "voyage-3",
        provider_id: "voyage",
        context_window: 32_000,
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
        id: "voyage-3-lite",
        provider_id: "voyage",
        context_window: 32_000,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Domain-specialized.
    ModelDef {
        id: "voyage-code-3",
        provider_id: "voyage",
        context_window: 32_000,
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
        id: "voyage-code-2",
        provider_id: "voyage",
        context_window: 16_000,
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
        id: "voyage-finance-2",
        provider_id: "voyage",
        context_window: 32_000,
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
        id: "voyage-law-2",
        provider_id: "voyage",
        context_window: 16_000,
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
        id: "voyage-multilingual-2",
        provider_id: "voyage",
        context_window: 32_000,
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
