use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "minimax",
    display_name: "MiniMax",
    // International OpenAI-compatible endpoint. The China region uses
    // https://api.minimaxi.chat/v1 with the same schema.
    default_base_url: "https://api.minimax.io/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["MINIMAX_API_KEY"],
    litellm_prefix: "minimax/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        tool_choice: false,
        embeddings: false,
        // MiniMax OpenAI-compat chat API explicitly does not accept image or
        // audio inputs as of the M2.x lineup.
        vision: false,
        batch: false,
    },
};

// GA text models on the international platform. All advertise a 204,800-token
// context window and up to 128k max output (including chain-of-thought).
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "MiniMax-M2.7",
        provider_id: "minimax",
        context_window: 204_800,
        max_output_tokens: 131_072,
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
        id: "MiniMax-M2.7-highspeed",
        provider_id: "minimax",
        context_window: 204_800,
        max_output_tokens: 131_072,
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
        id: "MiniMax-M2.5",
        provider_id: "minimax",
        context_window: 204_800,
        max_output_tokens: 131_072,
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
        id: "MiniMax-M2.5-highspeed",
        provider_id: "minimax",
        context_window: 204_800,
        max_output_tokens: 131_072,
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
        id: "MiniMax-M2.1",
        provider_id: "minimax",
        context_window: 204_800,
        max_output_tokens: 131_072,
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
        id: "MiniMax-M2.1-highspeed",
        provider_id: "minimax",
        context_window: 204_800,
        max_output_tokens: 131_072,
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
        id: "MiniMax-M2",
        provider_id: "minimax",
        context_window: 204_800,
        max_output_tokens: 131_072,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
];
