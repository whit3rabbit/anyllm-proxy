use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "aleph_alpha",
    display_name: "Aleph Alpha",
    default_base_url: "https://api.aleph-alpha.com",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["ALEPH_ALPHA_API_KEY"],
    litellm_prefix: "aleph_alpha/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: false,
        embeddings: true,
        vision: false,
        batch: false,
    },
};

// PhariaInference models exposed via the OpenAI-compatible /chat/completions
// endpoint on https://api.aleph-alpha.com. Pre-training sequence length is
// 8192 tokens; max_output_tokens mirrors that upper bound since the API does
// not publish a separate generation cap.
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "pharia-1-llm-7b-control",
        provider_id: "aleph_alpha",
        context_window: 8_192,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "pharia-1-llm-7b-control-aligned",
        provider_id: "aleph_alpha",
        context_window: 8_192,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
