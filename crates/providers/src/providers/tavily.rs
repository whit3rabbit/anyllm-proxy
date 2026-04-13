use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Tavily — AI-optimized search API. Chat completions not supported.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "tavily",
    display_name: "Tavily",
    default_base_url: "https://api.tavily.com",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["TAVILY_API_KEY"],
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
