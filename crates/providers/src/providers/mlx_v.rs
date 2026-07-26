use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "mlx_v",
    display_name: "mlx-v",
    default_base_url: "http://localhost:8080/v1",
    protocol: ProviderProtocol::OpenAICompat,
    // `vlm serve --api-key` gates only the management routes (/metrics,
    // /cache/*, /unload). Chat completions are open.
    auth: AuthKind::None,
    status: ProviderStatus::Stub,
    env_vars: &[],
    litellm_prefix: "mlx_v/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        // Only "auto" and "none" are honored; forcing a call needs
        // constrained decoding, which mlx-v rejects with a 400.
        tool_choice: false,
        // No /v1/embeddings route exists. Unlike lm_studio and ollama, which
        // advertise it, mlx-v is an inference toolkit with a chat surface
        // only.
        embeddings: false,
        vision: true,
        batch: false,
    },
};

// One process serves one model, named by `--model`, so there is no fixed
// catalog to enumerate.
pub const MODELS: &[ModelDef] = &[];
