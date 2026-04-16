use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Play.ht / PlayAI — text-to-speech and voice cloning. Chat completions not supported.
///
/// Docs: https://docs.play.ht (legacy v2) and https://docs.play.ai (newer unified API).
/// Base URL preserved as the v2 TTS endpoint `api.play.ht/api/v2`; PlayAI's `api.play.ai/api/v1`
/// is the successor surface (same vendor) and exposes `/tts`, `/tts/stream`, `/voices`,
/// plus a WebSocket TTS channel. Pick the v2 URL here because existing LiteLLM-style configs
/// reference it; callers targeting PlayAI can override `default_base_url` at config time.
///
/// NOTE on auth: Play.ht / PlayAI require TWO headers — `Authorization: <secret>` AND
/// `X-User-ID: <user-id>` (PlayAI uppercases it as `X-USER-ID`). `AuthKind::Bearer` is a
/// lossy fit: it only models a single bearer-style header and cannot convey the user-id
/// second factor. Wiring this provider for real traffic will require either a new
/// `AuthKind` variant (e.g. `BearerPlusUserId`) or a provider-specific header injector.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "playht",
    display_name: "Play.ht",
    default_base_url: "https://api.play.ht/api/v2",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Wired,
    env_vars: &["PLAYHT_SECRET_KEY", "PLAYHT_USER_ID", "PLAYAI_API_KEY"],
    litellm_prefix: "",
    capabilities: ProviderCapabilities {
        chat_completions: false,
        streaming: true,
        tool_use: false,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

// Model IDs follow the voice-engine names accepted by the PlayHT v2 `/tts` and
// `/tts/stream` endpoints (field `voice_engine`). PlayAI exposes the same
// engines under the names Dialog 1.0, Dialog 1.0 Turbo, and Play 3.0 Mini.
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "PlayDialog",
        provider_id: "playht",
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
        id: "PlayDialog-turbo",
        provider_id: "playht",
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
        id: "PlayDialogMultilingual",
        provider_id: "playht",
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
        id: "Play3.0-mini",
        provider_id: "playht",
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
        id: "PlayHT2.0",
        provider_id: "playht",
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
        id: "PlayHT2.0-turbo",
        provider_id: "playht",
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
        id: "PlayHT1.0",
        provider_id: "playht",
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
];
