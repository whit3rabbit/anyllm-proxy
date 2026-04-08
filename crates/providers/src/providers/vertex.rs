use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "vertex_ai",
    display_name: "Google Vertex AI",
    // URL is constructed per-project/region: https://{region}-aiplatform.googleapis.com/...
    default_base_url: "",
    protocol: ProviderProtocol::VertexAI,
    auth: AuthKind::GoogleApiKey,
    status: ProviderStatus::Implemented,
    env_vars: &["VERTEX_API_KEY", "GOOGLE_ACCESS_TOKEN"],
    litellm_prefix: "vertex_ai/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: true,
        vision: true,
        batch: false,
    },
};

// Vertex AI serves Gemini models (and others) under the same model IDs as Google AI Studio,
// routed via project/region endpoints. No separate model list needed.
pub const MODELS: &[ModelDef] = &[];
