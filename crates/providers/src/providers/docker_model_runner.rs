use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Docker Model Runner — Docker-native local model serving (no auth).
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "docker_model_runner",
    display_name: "Docker Model Runner",
    default_base_url: "http://localhost:12434/engines/llama.cpp/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::None,
    status: ProviderStatus::Stub,
    env_vars: &[],
    litellm_prefix: "docker_model_runner/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: false,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[];
