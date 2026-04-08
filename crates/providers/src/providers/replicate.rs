use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "replicate",
    display_name: "Replicate",
    default_base_url: "https://openai-compat.replicate.com/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["REPLICATE_API_KEY"],
    litellm_prefix: "replicate/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: false,
        embeddings: false,
        vision: true,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[];
