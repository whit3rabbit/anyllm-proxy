use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Pollinations — free, anonymous aggregate of text/image/audio generation
/// exposed via an OpenAI-compatible endpoint at `text.pollinations.ai/openai`.
///
/// Anonymous tier requires no API key (rate limited to roughly one request
/// per 15s). Registered users can pass a bearer token from auth.pollinations.ai
/// or authenticate web apps via the `referrer` parameter.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "pollinations",
    display_name: "Pollinations",
    default_base_url: "https://text.pollinations.ai/openai",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::None,
    status: ProviderStatus::Stub,
    env_vars: &[],
    litellm_prefix: "",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        tool_choice: false,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

/// Pollinations exposes friendly string aliases. The backing model behind an
/// alias can rotate over time as Pollinations swaps providers (OVH, OpenAI,
/// etc.). Context windows below are conservative estimates; the live
/// `/models` endpoint does not publish per-model windows.
///
/// Current authoritative anonymous listing at `GET https://text.pollinations.ai/models`
/// is just `openai-fast` (GPT-OSS 20B via OVH) with aliases `openai`,
/// `gpt-oss`, `gpt-oss-20b`, `ovh-reasoning`. Other ids below are documented
/// in APIDOCS.md for higher tiers or may require a bearer token.
pub const MODELS: &[ModelDef] = &[
    // Default anonymous-tier text model. Reasoning + tools enabled per live
    // /models response. Aliases: openai, gpt-oss, gpt-oss-20b, ovh-reasoning.
    ModelDef {
        id: "openai-fast",
        provider_id: "pollinations",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // Alias for openai-fast kept as an explicit entry so lookups by "openai"
    // resolve to known metadata.
    ModelDef {
        id: "openai",
        provider_id: "pollinations",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // Documented reasoning-focused variant (o4-mini class per APIDOCS.md).
    ModelDef {
        id: "openai-reasoning",
        provider_id: "pollinations",
        context_window: 128_000,
        max_output_tokens: 16_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // Web-search augmented chat model.
    ModelDef {
        id: "searchgpt",
        provider_id: "pollinations",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Audio-capable chat model (TTS voices: alloy, echo, fable, onyx, nova,
    // shimmer). Treated as stub: requires higher tier than anonymous.
    ModelDef {
        id: "openai-audio",
        provider_id: "pollinations",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Stub,
    },
    // Mistral-backed alias. Retained as Stub because not present in current
    // anonymous /models response but still referenced in APIDOCS.md examples.
    ModelDef {
        id: "mistral",
        provider_id: "pollinations",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Stub,
    },
];
