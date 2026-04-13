use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// AssemblyAI — audio intelligence and speech-to-text. Chat completions not supported.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "assemblyai",
    display_name: "AssemblyAI",
    default_base_url: "https://api.assemblyai.com",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Wired,
    env_vars: &["ASSEMBLYAI_API_KEY"],
    litellm_prefix: "",
    capabilities: ProviderCapabilities {
        chat_completions: false,
        streaming: false,
        tool_use: false,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "best",
        provider_id: "assemblyai",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "nano",
        provider_id: "assemblyai",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "conformer-2",
        provider_id: "assemblyai",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // LeMUR audio LLM for audio intelligence tasks (question answering, summaries, etc.)
    ModelDef {
        id: "slam-1",
        provider_id: "assemblyai",
        context_window: 0,
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
