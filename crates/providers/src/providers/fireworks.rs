use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "fireworks_ai",
    display_name: "Fireworks AI",
    default_base_url: "https://api.fireworks.ai/inference/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["FIREWORKS_API_KEY"],
    litellm_prefix: "fireworks_ai/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        tool_choice: false,
        embeddings: true,
        vision: false,
        batch: false,
    },
};

// Serverless GA models on Fireworks AI. IDs are the full
// `accounts/fireworks/models/<slug>` paths used directly in the `model` field
// of Chat Completions requests. Only publicly documented GA serverless
// deployments are listed; on-demand / dedicated deployments are omitted.
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "accounts/fireworks/models/llama-v3p3-70b-instruct",
        provider_id: "fireworks_ai",
        context_window: 131_072,
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
        id: "accounts/fireworks/models/llama-v3p1-405b-instruct",
        provider_id: "fireworks_ai",
        context_window: 131_072,
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
        id: "accounts/fireworks/models/llama-v3p1-8b-instruct",
        provider_id: "fireworks_ai",
        context_window: 131_072,
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
        id: "accounts/fireworks/models/deepseek-v3",
        provider_id: "fireworks_ai",
        context_window: 131_072,
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
        id: "accounts/fireworks/models/deepseek-r1",
        provider_id: "fireworks_ai",
        context_window: 163_840,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "accounts/fireworks/models/qwen2p5-72b-instruct",
        provider_id: "fireworks_ai",
        context_window: 32_768,
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
        id: "accounts/fireworks/models/qwen2p5-coder-32b-instruct",
        provider_id: "fireworks_ai",
        context_window: 32_768,
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
        id: "accounts/fireworks/models/mixtral-8x7b-instruct",
        provider_id: "fireworks_ai",
        context_window: 32_768,
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
        id: "accounts/fireworks/models/mixtral-8x22b-instruct",
        provider_id: "fireworks_ai",
        context_window: 65_536,
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
];
