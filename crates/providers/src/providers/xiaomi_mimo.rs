use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

// Xiaomi operates an official OpenAI-compatible API platform at
// `api.xiaomimimo.com` (developer portal at platform.xiaomimimo.com).
// Open weights are published under huggingface.co/XiaomiMiMo (MIT/Apache-2.0),
// but the hosted API is the canonical inference endpoint used here.
// Auth: `Authorization: Bearer <key>` issued from the platform dashboard.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "xiaomi_mimo",
    display_name: "Xiaomi MiMo",
    default_base_url: "https://api.xiaomimimo.com/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["XIAOMI_MIMO_API_KEY", "MIMO_API_KEY"],
    litellm_prefix: "xiaomi_mimo/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        tool_choice: false,
        embeddings: false,
        // mimo-v2-omni supports image input; advertise vision at provider level.
        vision: true,
        batch: false,
    },
};

// Model specs per Xiaomi platform docs (platform.xiaomimimo.com) and the
// MiMo-V2-Flash technical report. Context windows: Flash 262,144, Pro
// 1,048,576, Omni 262,144. Max output tokens from the same source.
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "mimo-v2-flash",
        provider_id: "xiaomi_mimo",
        context_window: 262_144,
        max_output_tokens: 8_192,
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
        id: "mimo-v2-pro",
        provider_id: "xiaomi_mimo",
        context_window: 1_048_576,
        max_output_tokens: 32_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "mimo-v2-omni",
        provider_id: "xiaomi_mimo",
        context_window: 262_144,
        max_output_tokens: 32_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
];
