use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// IBM WatsonX — OpenAI-compatible endpoint; set instance URL via WATSONX_URL.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "watsonx",
    display_name: "IBM WatsonX",
    default_base_url: "",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["WATSONX_API_KEY", "WATSONX_URL"],
    litellm_prefix: "watsonx/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: true,
        vision: false,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[];
