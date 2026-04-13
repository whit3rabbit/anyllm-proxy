use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// iFlytek Spark — Chinese LLM with an OpenAI-compatible endpoint.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "iflytek",
    display_name: "iFlytek Spark",
    default_base_url: "https://spark-api-open.xf-yun.com/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Wired,
    env_vars: &["SPARK_API_KEY", "IFLYTEK_API_KEY"],
    litellm_prefix: "spark/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: false,
        vision: true,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[
    // Spark 4.0 Ultra — flagship, 128k context
    ModelDef {
        id: "4.0Ultra",
        provider_id: "iflytek",
        context_window: 128_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Spark Max (generalv3.5)
    ModelDef {
        id: "generalv3.5",
        provider_id: "iflytek",
        context_window: 8_192,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Spark Pro (generalv3)
    ModelDef {
        id: "generalv3",
        provider_id: "iflytek",
        context_window: 8_192,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Spark V2 (general) — legacy
    ModelDef {
        id: "general",
        provider_id: "iflytek",
        context_window: 4_096,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Spark Lite — fastest, lowest cost
    ModelDef {
        id: "lite",
        provider_id: "iflytek",
        context_window: 4_096,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
