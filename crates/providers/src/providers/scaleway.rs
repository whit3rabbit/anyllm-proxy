use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Scaleway Generative APIs — OpenAI-compatible endpoint at api.scaleway.ai/v1.
/// Auth: `Authorization: Bearer $SCW_SECRET_KEY`.
/// Reference: https://www.scaleway.com/en/docs/generative-apis/reference-content/supported-models/
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "scaleway",
    display_name: "Scaleway",
    default_base_url: "https://api.scaleway.ai",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["SCW_SECRET_KEY"],
    litellm_prefix: "scaleway/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: true,
        // Pixtral (vision) is GA on Scaleway. Batch is not offered.
        vision: true,
        batch: false,
    },
};

// GA models only. Preview entries (e.g. gemma-3-27b-it) and non-commercial-only
// licences (e.g. holo2-30b-a3b / CC-BY-NC) are excluded. Context / output sizes
// come from the supported-models reference page (k = 1024 where the docs use it
// colloquially; values rounded to the published figure).
pub const MODELS: &[ModelDef] = &[
    // Qwen flagship instruction model (best-accuracy recommendation from Scaleway).
    ModelDef {
        id: "qwen3.5-397b-a17b",
        provider_id: "scaleway",
        context_window: 250_000,
        max_output_tokens: 16_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "qwen3-235b-a22b-instruct-2507",
        provider_id: "scaleway",
        context_window: 250_000,
        max_output_tokens: 16_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "qwen3-coder-30b-a3b-instruct",
        provider_id: "scaleway",
        context_window: 128_000,
        max_output_tokens: 32_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Mistral — getting-started recommendation from Scaleway.
    ModelDef {
        id: "mistral-small-3.2-24b-instruct-2506",
        provider_id: "scaleway",
        context_window: 128_000,
        max_output_tokens: 32_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "devstral-2-123b-instruct-2512",
        provider_id: "scaleway",
        context_window: 200_000,
        max_output_tokens: 16_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "mistral-nemo-instruct-2407",
        provider_id: "scaleway",
        context_window: 128_000,
        max_output_tokens: 8_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // OpenAI open-weight.
    ModelDef {
        id: "gpt-oss-120b",
        provider_id: "scaleway",
        context_window: 128_000,
        max_output_tokens: 32_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Meta Llama (3.x).
    ModelDef {
        id: "llama-3.3-70b-instruct",
        provider_id: "scaleway",
        context_window: 100_000,
        max_output_tokens: 16_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "llama-3.1-8b-instruct",
        provider_id: "scaleway",
        context_window: 128_000,
        max_output_tokens: 16_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // DeepSeek reasoning distill (thinking-capable).
    ModelDef {
        id: "deepseek-r1-distill-llama-70b",
        provider_id: "scaleway",
        context_window: 16_000,
        max_output_tokens: 4_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // Vision: Pixtral.
    ModelDef {
        id: "pixtral-12b-2409",
        provider_id: "scaleway",
        context_window: 128_000,
        max_output_tokens: 4_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Embedding models (max_output_tokens = 0 by convention; context = max input tokens).
    ModelDef {
        id: "qwen3-embedding-8b",
        provider_id: "scaleway",
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
        id: "bge-multilingual-gemma2",
        provider_id: "scaleway",
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
