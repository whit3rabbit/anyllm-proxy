use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// AI21 Studio via their OpenAI-compatible chat completions endpoint.
/// Base: https://api.ai21.com/studio/v1, path: /chat/completions, auth: Bearer.
/// Native AI21 format is not implemented.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "ai21",
    display_name: "AI21 Labs",
    default_base_url: "https://api.ai21.com/studio/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["AI21_API_KEY"],
    litellm_prefix: "ai21/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        // Jamba chat API accepts a `tools` array (function-type only).
        tool_use: true,
        tool_choice: false,
        // Legacy J2 embed endpoint is no longer a listed Studio product.
        embeddings: false,
        // No image/vision input documented for Jamba chat.
        vision: false,
        // No public batch API.
        batch: false,
    },
};

// Public GA Jamba chat models. Both advertise a 256K context window;
// the chat API caps `max_tokens` at 4096 per response. Streaming works,
// but Jamba docs note streaming cannot be combined with tools.
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "jamba-large",
        provider_id: "ai21",
        context_window: 256_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "jamba-mini",
        provider_id: "ai21",
        context_window: 256_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
