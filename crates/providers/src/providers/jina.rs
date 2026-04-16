use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Jina AI — embeddings, reranking, reader, classifier, and segmenter APIs.
///
/// Not a chat LLM. OpenAI-compatible `/v1/embeddings` endpoint plus
/// Jina-specific `/v1/rerank`, `/v1/classify`, `/v1/segment`, and reader
/// endpoints. All routes share the `api.jina.ai` host and Bearer auth.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "jina",
    display_name: "Jina AI",
    default_base_url: "https://api.jina.ai/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["JINA_AI_API_KEY"],
    litellm_prefix: "jina_ai/",
    capabilities: ProviderCapabilities {
        chat_completions: false,
        streaming: false,
        tool_use: false,
        embeddings: true,
        vision: false,
        batch: false,
    },
};

// Context windows sourced from jina.ai/embeddings and jina.ai/reranker (GA models).
// `max_output_tokens` is not meaningful for embeddings/reranking; set to 0.
pub const MODELS: &[ModelDef] = &[
    // Embeddings
    ModelDef {
        id: "jina-embeddings-v4",
        provider_id: "jina",
        context_window: 32_768,
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
        id: "jina-embeddings-v3",
        provider_id: "jina",
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
    ModelDef {
        id: "jina-clip-v2",
        provider_id: "jina",
        context_window: 8_192,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Rerankers
    ModelDef {
        id: "jina-reranker-v2-base-multilingual",
        provider_id: "jina",
        context_window: 1_024,
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
        id: "jina-colbert-v2",
        provider_id: "jina",
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
