use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "moonshot",
    display_name: "Moonshot AI",
    // International endpoint. China-region users should override to
    // https://api.moonshot.cn/v1 via config.
    default_base_url: "https://api.moonshot.ai/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["MOONSHOT_API_KEY"],
    litellm_prefix: "moonshot/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: false,
        // kimi-k2.5 is natively multimodal (text + image) and GA.
        vision: true,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[
    // Kimi K2.5: GA trillion-parameter flagship with 256K context and native
    // multimodal (text + image) input. Supports tool calling and streaming.
    ModelDef {
        id: "kimi-k2.5",
        provider_id: "moonshot",
        context_window: 256_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Moonshot V1 series: GA text-only chat models. Tool calling and streaming
    // supported. Vision is a separate `-vision-preview` variant (not GA, omitted).
    // Moonshot does not publish a fixed max_output cap; 4096 is the documented
    // default ceiling for `max_tokens` across the v1 family.
    ModelDef {
        id: "moonshot-v1-8k",
        provider_id: "moonshot",
        context_window: 8_192,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "moonshot-v1-32k",
        provider_id: "moonshot",
        context_window: 32_768,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "moonshot-v1-128k",
        provider_id: "moonshot",
        context_window: 131_072,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // moonshot-v1-auto: server-side router that picks the smallest v1 variant
    // that fits the prompt. Context reported as the largest backing model (128K).
    ModelDef {
        id: "moonshot-v1-auto",
        provider_id: "moonshot",
        context_window: 131_072,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
