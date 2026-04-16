use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Stability AI — image generation (Stable Image / Stable Diffusion 3.5 family)
/// plus upscalers, edits, and video endpoints. All endpoints are per-model REST
/// paths under `/v2beta` (e.g. `POST /v2beta/stable-image/generate/ultra`),
/// not OpenAI-compatible chat. See https://platform.stability.ai/docs/api-reference.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "stability_ai",
    display_name: "Stability AI",
    // Official REST host. Path versioning (`/v2beta/...`, `/v1/...`) is per-endpoint
    // and therefore handled by the caller, not baked into the base URL.
    default_base_url: "https://api.stability.ai",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Wired,
    env_vars: &["STABILITY_API_KEY"],
    litellm_prefix: "stability_ai/",
    capabilities: ProviderCapabilities {
        // Image-generation focused: no chat, streaming, tools, embeddings, or batch.
        // `vision` here means input-image-as-context for chat, which Stability does
        // not offer (image-to-image exists, but is a separate REST endpoint, not a
        // chat vision capability).
        chat_completions: false,
        streaming: false,
        tool_use: false,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

// Stability's API exposes per-endpoint model paths (e.g. `/v2beta/stable-image/
// generate/ultra`) rather than a `model` field in a chat request, so there are
// no chat-style model IDs to enumerate here. Consumers select behaviour by URL.
pub const MODELS: &[ModelDef] = &[];
