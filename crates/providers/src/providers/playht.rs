use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Play.ht — text-to-speech and voice cloning. Chat completions not supported.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "playht",
    display_name: "Play.ht",
    default_base_url: "https://api.play.ht/api/v2",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Wired,
    env_vars: &["PLAYHT_SECRET_KEY"],
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
];
