use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Blackbox AI — LLM chat service at blackbox.ai with an OpenAI-compatible endpoint.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "blackboxai",
    display_name: "Blackbox AI",
    default_base_url: "https://api.blackbox.ai/api",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Wired,
    env_vars: &["BLACKBOXAI_API_KEY"],
    litellm_prefix: "",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: false,
        embeddings: false,
        vision: true,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[
    // Blackbox native model
    ModelDef {
        id: "blackboxai",
        provider_id: "blackboxai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // GPT-4o proxy
    ModelDef {
        id: "gpt-4o",
        provider_id: "blackboxai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Claude 3 Opus proxy
    ModelDef {
        id: "claude-3-opus",
        provider_id: "blackboxai",
        context_window: 200_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Gemini Pro proxy
    ModelDef {
        id: "gemini-pro",
        provider_id: "blackboxai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Llama 3.1 proxy
    ModelDef {
        id: "llama-3.1-8b",
        provider_id: "blackboxai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "llama-3.1-70b",
        provider_id: "blackboxai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // DeepSeek V3 proxy
    ModelDef {
        id: "deepseek-v3",
        provider_id: "blackboxai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // DeepSeek R1 proxy
    ModelDef {
        id: "deepseek-r1",
        provider_id: "blackboxai",
        context_window: 128_000,
        max_output_tokens: 16_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
];
