use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Lemonade — AMD's local LLM server with an OpenAI-compatible API.
/// Default port 13305; users load their own models via `/api/v1/pull` + `/api/v1/load`.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "lemonade",
    display_name: "Lemonade",
    default_base_url: "http://localhost:13305/api/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::None,
    status: ProviderStatus::Stub,
    env_vars: &[],
    litellm_prefix: "lemonade/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        tool_choice: false,
        embeddings: true,
        vision: true,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[];
