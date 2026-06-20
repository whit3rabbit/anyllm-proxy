use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

// Alibaba Cloud Model Studio (DashScope), Qwen family.
// OpenAI-compatible endpoint: https://dashscope.aliyuncs.com/compatible-mode/v1
// International (Singapore): https://dashscope-intl.aliyuncs.com/compatible-mode/v1
// US (Virginia):             https://dashscope-us.aliyuncs.com/compatible-mode/v1
// Auth: Bearer $DASHSCOPE_API_KEY.
// Only publicly GA commercial + GA open-source models are listed below.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "dashscope",
    display_name: "Dashscope (Qwen)",
    default_base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["DASHSCOPE_API_KEY"],
    litellm_prefix: "dashscope/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        tool_choice: false,
        embeddings: true,
        vision: true,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[
    // Commercial flagship, stable aliases.
    ModelDef {
        id: "qwen-max",
        provider_id: "dashscope",
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
        id: "qwen-plus",
        provider_id: "dashscope",
        context_window: 131_072,
        max_output_tokens: 8_192,
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
        id: "qwen-turbo",
        provider_id: "dashscope",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // Long-context document model. Up to ~10M tokens via the long-context endpoint;
    // keeping a conservative figure that matches the commercial OpenAI-compat default.
    ModelDef {
        id: "qwen-long",
        provider_id: "dashscope",
        context_window: 10_000_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Vision-language flagships.
    ModelDef {
        id: "qwen-vl-max",
        provider_id: "dashscope",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "qwen-vl-plus",
        provider_id: "dashscope",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Open-source Qwen2.5 series (served via DashScope).
    ModelDef {
        id: "qwen2.5-72b-instruct",
        provider_id: "dashscope",
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
        id: "qwen2.5-32b-instruct",
        provider_id: "dashscope",
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
        id: "qwen2.5-14b-instruct",
        provider_id: "dashscope",
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
        id: "qwen2.5-7b-instruct",
        provider_id: "dashscope",
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
        id: "qwen2.5-coder-32b-instruct",
        provider_id: "dashscope",
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
    // Open-source Qwen2.5-VL.
    ModelDef {
        id: "qwen2.5-vl-72b-instruct",
        provider_id: "dashscope",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "qwen2.5-vl-7b-instruct",
        provider_id: "dashscope",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // QwQ reasoning model.
    ModelDef {
        id: "qwq-32b",
        provider_id: "dashscope",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // Embeddings. v3 is GA; v4 (Qwen3-Embedding) is also GA per DashScope docs.
    ModelDef {
        id: "text-embedding-v3",
        provider_id: "dashscope",
        context_window: 8_192,
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
    ModelDef {
        id: "text-embedding-v4",
        provider_id: "dashscope",
        context_window: 8_192,
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
