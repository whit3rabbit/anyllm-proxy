use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// ElevenLabs — text-to-speech, speech-to-text, and voice AI.
///
/// Base URL: `https://api.elevenlabs.io/v1` (TTS `/text-to-speech/{voice_id}`,
/// STT `/speech-to-text`, STS `/speech-to-speech/{voice_id}`, etc.).
///
/// Auth: `xi-api-key: <key>` HTTP header — NOT `Authorization: Bearer`.
/// `AuthKind::Bearer` below is wrong; the `AuthKind` enum has no `XiApiKey`
/// variant yet. Adding one (and plumbing it through the HTTP clients) is out
/// of scope for this metadata update. Flag when wiring a real client.
///
/// Chat completions are not supported. Streaming is available on most TTS
/// endpoints (chunked audio + websocket `/v1/text-to-speech/{voice_id}/stream`).
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "elevenlabs",
    display_name: "ElevenLabs",
    default_base_url: "https://api.elevenlabs.io/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Wired,
    env_vars: &["ELEVENLABS_API_KEY"],
    litellm_prefix: "",
    capabilities: ProviderCapabilities {
        chat_completions: false,
        streaming: true,
        tool_use: false,
        tool_choice: false,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[
    // TTS — current generation
    ModelDef {
        id: "eleven_v3",
        provider_id: "elevenlabs",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "eleven_multilingual_v2",
        provider_id: "elevenlabs",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "eleven_flash_v2_5",
        provider_id: "elevenlabs",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "eleven_flash_v2",
        provider_id: "elevenlabs",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "eleven_turbo_v2_5",
        provider_id: "elevenlabs",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "eleven_turbo_v2",
        provider_id: "elevenlabs",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // TTS — legacy
    ModelDef {
        id: "eleven_monolingual_v1",
        provider_id: "elevenlabs",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Deprecated,
    },
    ModelDef {
        id: "eleven_multilingual_v1",
        provider_id: "elevenlabs",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Deprecated,
    },
    // Speech-to-speech (voice conversion)
    ModelDef {
        id: "eleven_multilingual_sts_v2",
        provider_id: "elevenlabs",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "eleven_english_sts_v2",
        provider_id: "elevenlabs",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Speech-to-text
    ModelDef {
        id: "scribe_v1",
        provider_id: "elevenlabs",
        context_window: 0,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
