use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Xinference — self-hosted inference server; set endpoint via XINFERENCE_SERVER_URL.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "xinference",
    display_name: "Xinference",
    default_base_url: "",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::None,
    status: ProviderStatus::Stub,
    env_vars: &["XINFERENCE_SERVER_URL"],
    litellm_prefix: "xinference/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: false,
        embeddings: true,
        vision: false,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[];
