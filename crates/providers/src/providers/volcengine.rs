use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Volcano Engine (ByteDance Ark) — OpenAI-compatible endpoint.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "volcengine",
    display_name: "Volcano Engine",
    default_base_url: "https://ark.cn-beijing.volces.com/api/v3",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["VOLCENGINE_API_KEY"],
    litellm_prefix: "volcengine/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: false,
        vision: true,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[];
