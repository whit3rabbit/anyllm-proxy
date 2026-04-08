use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Voyage AI — embeddings-only provider.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "voyage",
    display_name: "Voyage AI",
    default_base_url: "https://api.voyageai.com/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["VOYAGE_API_KEY"],
    litellm_prefix: "voyage/",
    capabilities: ProviderCapabilities {
        chat_completions: false,
        streaming: false,
        tool_use: false,
        embeddings: true,
        vision: false,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[];
