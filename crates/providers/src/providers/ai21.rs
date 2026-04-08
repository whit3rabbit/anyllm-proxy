use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// AI21 via their OpenAI-compatible endpoint.
/// Native AI21 format is not implemented.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "ai21",
    display_name: "AI21 Labs",
    default_base_url: "https://api.ai21.com/studio/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["AI21_API_KEY"],
    litellm_prefix: "ai21/",
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
