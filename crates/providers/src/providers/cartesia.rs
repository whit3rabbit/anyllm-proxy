use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

// Cartesia: real-time voice AI (TTS + STT). Not a chat provider.
// Base URL: https://api.cartesia.ai
// Auth header is X-API-Key (not Bearer); AuthKind enum lacks that variant,
// so we keep Bearer as the closest placeholder until the enum is extended.
// TTS endpoints (/tts/bytes, /tts/sse, /tts/websocket) support real-time streaming.
// STT via Ink-Whisper exposes /stt (native) and /audio/transcriptions (OpenAI-compat).
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "cartesia",
    display_name: "Cartesia",
    default_base_url: "https://api.cartesia.ai",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Wired,
    env_vars: &["CARTESIA_API_KEY"],
    litellm_prefix: "",
    capabilities: ProviderCapabilities {
        chat_completions: false,
        // Sonic family streams first audio bytes in 40-90ms; streaming is the core use case.
        streaming: true,
        tool_use: false,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[
    // TTS: Sonic-3 (current flagship, 90ms TTFB, 40+ languages, expressive laughter).
    ModelDef {
        id: "sonic-3",
        provider_id: "cartesia",
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
    // TTS: Sonic-2 (latency-optimised, best-in-class voice cloning).
    ModelDef {
        id: "sonic-2",
        provider_id: "cartesia",
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
    // TTS: Sonic-2 pinned snapshot, kept available for users needing _experimental_controls
    // (removed in snapshots after 2025-03-07).
    ModelDef {
        id: "sonic-2-2025-03-07",
        provider_id: "cartesia",
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
    // TTS: Sonic-2 latest production-pinned snapshot per docs.
    ModelDef {
        id: "sonic-2-2025-06-11",
        provider_id: "cartesia",
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
    // TTS: Sonic Turbo (40ms first-byte latency, real-time priority).
    ModelDef {
        id: "sonic-turbo",
        provider_id: "cartesia",
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
    // TTS: original Sonic base alias (kept for backward compatibility).
    ModelDef {
        id: "sonic",
        provider_id: "cartesia",
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
    // STT: Ink-Whisper (streaming and batch transcription, conversational-AI tuned).
    ModelDef {
        id: "ink-whisper",
        provider_id: "cartesia",
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
    // Legacy TTS snapshot alias, superseded by sonic-2 family.
    ModelDef {
        id: "sonic-2024-10-19",
        provider_id: "cartesia",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Deprecated,
    },
    // Legacy language-specific aliases, replaced by sonic-2's multilingual default.
    ModelDef {
        id: "sonic-english",
        provider_id: "cartesia",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Deprecated,
    },
    ModelDef {
        id: "sonic-multilingual",
        provider_id: "cartesia",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Deprecated,
    },
    ModelDef {
        id: "upbeat-moon",
        provider_id: "cartesia",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Deprecated,
    },
];
