use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Weights & Biases Inference — per-project URL; set via api_base or OPENAI_BASE_URL.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "wandb",
    display_name: "Weights & Biases Inference",
    default_base_url: "",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["WANDB_API_KEY"],
    litellm_prefix: "wandb/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: false,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[];
