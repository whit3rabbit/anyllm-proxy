use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// OVHCloud AI Endpoints — per-deployment URL; set via api_base or OPENAI_BASE_URL.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "ovhcloud",
    display_name: "OVHCloud AI Endpoints",
    default_base_url: "",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["OVH_AI_ENDPOINTS_ACCESS_TOKEN"],
    litellm_prefix: "ovhcloud/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[];
