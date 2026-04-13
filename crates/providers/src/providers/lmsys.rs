use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// LMSYS — self-hosted FastChat server (Vicuna and other open models).
/// Set api_base in the managed backend config to point to your FastChat instance.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "lmsys",
    display_name: "LMSYS (FastChat)",
    default_base_url: "http://localhost:8000/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::None,
    status: ProviderStatus::Stub,
    env_vars: &[],
    litellm_prefix: "",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: false,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

/// These are documentation-only entries for models commonly run via FastChat.
/// The actual available models depend entirely on what the user has loaded in
/// their FastChat instance. Override via managed backend config as needed.
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "vicuna-13b-v1.5",
        provider_id: "lmsys",
        context_window: 4_096,
        max_output_tokens: 2_048,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "vicuna-7b-v1.5",
        provider_id: "lmsys",
        context_window: 4_096,
        max_output_tokens: 2_048,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "llama-2-7b-chat",
        provider_id: "lmsys",
        context_window: 4_096,
        max_output_tokens: 2_048,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "llama-2-13b-chat",
        provider_id: "lmsys",
        context_window: 4_096,
        max_output_tokens: 2_048,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "mistral-7b-instruct",
        provider_id: "lmsys",
        context_window: 4_096,
        max_output_tokens: 2_048,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
