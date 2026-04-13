use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Exa — semantic search API for AI applications. Chat completions not supported.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "exa",
    display_name: "Exa",
    default_base_url: "https://api.exa.ai",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["EXA_API_KEY"],
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
