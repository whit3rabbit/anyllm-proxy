use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Codestral is Mistral's code-focused endpoint with a separate API key.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "codestral",
    display_name: "Codestral",
    default_base_url: "https://codestral.mistral.ai/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["CODESTRAL_API_KEY"],
    litellm_prefix: "codestral/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[];
