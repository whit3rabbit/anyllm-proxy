use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Scaleway Generative APIs — per-deployment endpoint; set base URL via config.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "scaleway",
    display_name: "Scaleway",
    default_base_url: "",
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
        vision: false,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[];
