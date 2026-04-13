use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Brave Search API — web and AI search. Chat completions not supported.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "brave",
    display_name: "Brave Search",
    default_base_url: "https://api.search.brave.com",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["BRAVE_API_KEY"],
    litellm_prefix: "",
    capabilities: ProviderCapabilities {
        chat_completions: false,
        streaming: false,
        tool_use: false,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

// No model selection — single endpoint API
pub const MODELS: &[ModelDef] = &[];
