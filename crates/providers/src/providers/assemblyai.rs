use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// AssemblyAI — speech-to-text and audio intelligence. Also ships an OpenAI-compatible
/// LLM Gateway (`POST /v1/chat/completions`) that proxies Claude, GPT, and Gemini with
/// audio context; this supersedes the now-deprecated LeMUR API.
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
        // LLM Gateway exposes OpenAI-compatible /v1/chat/completions.
        chat_completions: true,
        // Real-time STT (Universal Streaming) and LLM Gateway SSE both supported.
        streaming: true,
        // LLM Gateway supports tool calling.
        tool_use: true,
        embeddings: false,
        vision: false,
        // No OpenAI-style /v1/batches endpoint; concurrent transcription submissions only.
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[
    // Current STT models (2025). See:
    // https://www.assemblyai.com/docs/pre-recorded-audio/select-the-speech-model
    ModelDef {
        id: "universal-3-pro",
        provider_id: "assemblyai",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "universal-2",
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
    // Legacy speech_model tier slugs; still accepted by /v2/transcript for back-compat.
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
    // Speech-language-aware model for domain-specific transcription and fine-tuning.
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
