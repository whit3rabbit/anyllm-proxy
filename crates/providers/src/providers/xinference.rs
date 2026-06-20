use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Xinference (Xorbits Inference) — self-hosted inference server exposing an
/// OpenAI-compatible REST API at `/v1` (chat, embeddings, images, audio).
/// Default listener is `http://localhost:9997`; override via the proxy's
/// per-backend base URL config when running remotely.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "xinference",
    display_name: "Xinference",
    default_base_url: "http://localhost:9997/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::None,
    status: ProviderStatus::Stub,
    // Self-hosted: no API key required by default. Any non-empty string is
    // accepted if one is sent, so no canonical env var is defined.
    env_vars: &[],
    litellm_prefix: "xinference/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        // Xinference supports OpenAI-style function/tool calling for compatible models.
        tool_use: true,
        tool_choice: false,
        embeddings: true,
        // Vision-capable multimodal LLMs are supported when the user launches one.
        vision: true,
        // No server-side OpenAI-style batch API.
        batch: false,
    },
};

// Models are user-deployed at runtime (launched via Xinference's own API),
// so there is no static catalog to list here.
pub const MODELS: &[ModelDef] = &[];
