use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Azure AI Foundry (Serverless API / Models-as-a-Service).
/// Endpoint is per-deployment; set base URL via AZURE_AI_API_BASE or config.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "azure_ai",
    display_name: "Azure AI Foundry",
    default_base_url: "",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["AZURE_AI_API_KEY", "AZURE_AI_API_BASE"],
    litellm_prefix: "azure_ai/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: true,
        vision: true,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[];
