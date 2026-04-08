use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// HuggingFace Inference Endpoints (TGI / serverless inference) via OpenAI-compatible API.
/// Endpoint URL is per-deployment; set `OPENAI_BASE_URL` to override.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "huggingface",
    display_name: "HuggingFace",
    default_base_url: "",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["HUGGINGFACE_API_KEY", "HF_TOKEN"],
    litellm_prefix: "huggingface/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: false,
        embeddings: true,
        vision: false,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[];
