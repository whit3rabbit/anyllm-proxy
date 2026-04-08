use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "hyperbolic",
    display_name: "Hyperbolic",
    default_base_url: "https://api.hyperbolic.xyz/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["HYPERBOLIC_API_KEY"],
    litellm_prefix: "hyperbolic/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: false,
        vision: true,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[];
