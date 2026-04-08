use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Jina AI — embeddings and reranking provider.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "jina",
    display_name: "Jina AI",
    default_base_url: "https://api.jina.ai/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["JINA_AI_API_KEY"],
    litellm_prefix: "jina_ai/",
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
