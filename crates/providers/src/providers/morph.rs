use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "morph",
    display_name: "Morph",
    default_base_url: "https://api.morphllm.com/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["MORPH_API_KEY"],
    litellm_prefix: "morph/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        tool_choice: false,
        embeddings: true,
        vision: false,
        batch: false,
    },
};

// GA model catalog per https://docs.morphllm.com/llms.txt and /models/*.
// Morph specializes in fast code-edit "apply" models plus embeddings and rerank.
// Context windows / max output tokens are not published; left as 0 where unknown.
pub const MODELS: &[ModelDef] = &[
    // Apply models (code-edit). OpenAI-compatible chat/completions endpoint.
    ModelDef {
        id: "morph-v3-fast",
        provider_id: "morph",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "morph-v3-large",
        provider_id: "morph",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "auto",
        provider_id: "morph",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Embedding models. Non-chat endpoint; streaming/tool_use N/A.
    ModelDef {
        id: "morph-embedding-v3",
        provider_id: "morph",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "morph-embedding-v2",
        provider_id: "morph",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Rerank model. Non-chat endpoint.
    ModelDef {
        id: "morph-rerank-v3",
        provider_id: "morph",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
