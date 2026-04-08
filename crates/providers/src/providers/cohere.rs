use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Cohere via their OpenAI-compatible compatibility endpoint.
/// Native Cohere format (cohere_chat) is not implemented.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "cohere_chat",
    display_name: "Cohere",
    default_base_url: "https://api.cohere.com/compatibility/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["COHERE_API_KEY"],
    litellm_prefix: "cohere_chat/",
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
