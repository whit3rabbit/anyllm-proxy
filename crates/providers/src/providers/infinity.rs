use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Infinity — self-hosted OpenAI-compatible embeddings & reranking server
/// (michaelfeil/infinity). No auth by default; optional `INFINITY_API_KEY`.
/// Models are user-served, so the catalog is empty.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "infinity",
    display_name: "Infinity",
    default_base_url: "http://localhost:7997/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::None,
    status: ProviderStatus::Stub,
    env_vars: &[],
    litellm_prefix: "infinity/",
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
