use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Baseten — OpenAI-compatible inference.
///
/// Two surfaces share the same auth (Bearer <BASETEN_API_KEY>):
/// 1. Model APIs (shared hosted catalog): `https://inference.baseten.co/v1`.
///    Use this as the `default_base_url`; model IDs below target it.
/// 2. Per-deployment endpoints for custom models/chains:
///    `https://model-{model_id}.api.baseten.co/{environment}/sync/v1`
///    Users must override `base_url` in config when pointing at their own
///    deployment; the template is not representable here as a constant.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "baseten",
    display_name: "Baseten",
    default_base_url: "https://inference.baseten.co/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["BASETEN_API_KEY"],
    litellm_prefix: "baseten/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

// Baseten Model APIs catalog (publicly GA). Slugs match the HuggingFace-style
// model IDs accepted by `https://inference.baseten.co/v1/chat/completions`.
// Keep conservative: only include models documented as generally available.
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "deepseek-ai/DeepSeek-V3-0324",
        provider_id: "baseten",
        context_window: 164_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "deepseek-ai/DeepSeek-V3.1",
        provider_id: "baseten",
        context_window: 164_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "zai-org/GLM-4.6",
        provider_id: "baseten",
        context_window: 200_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "moonshotai/Kimi-K2.5",
        provider_id: "baseten",
        context_window: 262_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "MiniMaxAI/MiniMax-M2.5",
        provider_id: "baseten",
        context_window: 204_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "openai/gpt-oss-120b",
        provider_id: "baseten",
        context_window: 128_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
