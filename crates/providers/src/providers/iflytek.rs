use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// iFlytek Spark (讯飞星火) — Chinese LLM with an OpenAI-compatible HTTP endpoint.
///
/// OpenAI-compatible base: `https://spark-api-open.xf-yun.com/v1` (HTTP, Bearer APIPassword).
/// A separate native WebSocket API exists at `wss://spark-api.xf-yun.com/...` using
/// HMAC-SHA256 request signing; this metadata targets the HTTP/OpenAI-compat surface only.
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
        tool_choice: false,
        embeddings: false,
        // Spark text models do not document vision on the HTTP OpenAI-compat surface.
        vision: false,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[
    // Spark 4.0 Ultra — flagship. 32k context / 32k output per official HTTP docs.
    ModelDef {
        id: "4.0Ultra",
        provider_id: "iflytek",
        context_window: 32_768,
        max_output_tokens: 32_768,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Spark Max (generalv3.5) — 8k context, supports function calling and system prompts.
    ModelDef {
        id: "generalv3.5",
        provider_id: "iflytek",
        context_window: 8_192,
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
    // Spark Max-32K — extended-context variant of Max.
    ModelDef {
        id: "max-32k",
        provider_id: "iflytek",
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
    // Spark Pro (generalv3) — 8k context. Tool use not offered on Pro per HTTP docs.
    ModelDef {
        id: "generalv3",
        provider_id: "iflytek",
        context_window: 8_192,
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
    // Spark Pro-128K — long-context Pro variant.
    ModelDef {
        id: "pro-128k",
        provider_id: "iflytek",
        context_window: 131_072,
        max_output_tokens: 32_768,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Spark Lite — fastest, lowest cost. 8k context / 4k output.
    ModelDef {
        id: "lite",
        provider_id: "iflytek",
        context_window: 8_192,
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
];
