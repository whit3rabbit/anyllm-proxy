use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// GitHub Models — Azure-hosted OpenAI-compatible endpoint using a GitHub token.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "github",
    display_name: "GitHub Models",
    default_base_url: "https://models.inference.ai.azure.com",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["GITHUB_TOKEN"],
    litellm_prefix: "github/",
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
