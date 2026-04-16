use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Codestral is Mistral's code-focused endpoint with a separate API key.
/// Base URL serves FIM (`/v1/fim/completions`), chat (`/v1/chat/completions`),
/// and embeddings (`/v1/embeddings`). Auth is `Authorization: Bearer $CODESTRAL_API_KEY`.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "codestral",
    display_name: "Codestral",
    default_base_url: "https://codestral.mistral.ai/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["CODESTRAL_API_KEY"],
    litellm_prefix: "codestral/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: true,
        vision: false,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[
    // `codestral-latest` currently aliases the v25.08 release (July 2025).
    ModelDef {
        id: "codestral-latest",
        provider_id: "codestral",
        context_window: 256_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "codestral-2508",
        provider_id: "codestral",
        context_window: 256_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Semantic code embeddings. 8k input context, variable output dimensions.
    ModelDef {
        id: "codestral-embed-2505",
        provider_id: "codestral",
        context_window: 8_192,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
