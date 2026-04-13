use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Baidu ERNIE (Qianfan) — Chinese LLM platform.
/// Authentication uses AK/SK; set QIANFAN_AK and QIANFAN_SK.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "baidu",
    display_name: "Baidu ERNIE",
    default_base_url: "https://aip.baidubce.com/rpc/2.0/ai_custom/v1/wenxinworkshop",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Wired,
    env_vars: &["QIANFAN_AK", "QIANFAN_SK"],
    litellm_prefix: "qianfan/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: true,
        vision: true,
        batch: false,
    },
};

pub const MODELS: &[ModelDef] = &[
    // ERNIE 4.0 series
    ModelDef {
        id: "ernie-4.0-8k",
        provider_id: "baidu",
        context_window: 8_192,
        max_output_tokens: 2_048,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "ernie-4.0-8k-preview",
        provider_id: "baidu",
        context_window: 8_192,
        max_output_tokens: 2_048,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // ERNIE 3.5 series
    ModelDef {
        id: "ernie-3.5-8k",
        provider_id: "baidu",
        context_window: 8_192,
        max_output_tokens: 2_048,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "ernie-3.5-128k",
        provider_id: "baidu",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // ERNIE Speed series
    ModelDef {
        id: "ernie-speed-8k",
        provider_id: "baidu",
        context_window: 8_192,
        max_output_tokens: 2_048,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "ernie-speed-128k",
        provider_id: "baidu",
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
    // ERNIE Lite series
    ModelDef {
        id: "ernie-lite-8k",
        provider_id: "baidu",
        context_window: 8_192,
        max_output_tokens: 2_048,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // ERNIE Tiny
    ModelDef {
        id: "ernie-tiny-8k",
        provider_id: "baidu",
        context_window: 8_192,
        max_output_tokens: 2_048,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // ERNIE Character
    ModelDef {
        id: "ernie-character-8k",
        provider_id: "baidu",
        context_window: 8_192,
        max_output_tokens: 2_048,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // ERNIE with vision
    ModelDef {
        id: "ernie-4.0-turbo-8k",
        provider_id: "baidu",
        context_window: 8_192,
        max_output_tokens: 2_048,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Embedding
    ModelDef {
        id: "embedding-v1",
        provider_id: "baidu",
        context_window: 384,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "bce-embedding-base_v1",
        provider_id: "baidu",
        context_window: 512,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: false,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
