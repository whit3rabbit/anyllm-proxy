use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "cerebras",
    display_name: "Cerebras",
    default_base_url: "https://api.cerebras.ai/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["CEREBRAS_API_KEY"],
    litellm_prefix: "cerebras/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        tool_choice: false,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

// Cerebras Inference GA (production) models only.
// Preview models (e.g. qwen-3-235b-a22b-instruct-2507, zai-glm-4.7) are
// intentionally omitted per docs: "intended for evaluation purposes only and
// should not be used in production."
// Source: https://inference-docs.cerebras.ai/models/overview
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "llama3.1-8b",
        provider_id: "cerebras",
        context_window: 128_000,
        max_output_tokens: 8_192,
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
        id: "gpt-oss-120b",
        provider_id: "cerebras",
        context_window: 131_072,
        max_output_tokens: 32_768,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
