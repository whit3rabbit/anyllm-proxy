use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Baidu ERNIE (Qianfan) — Chinese LLM platform.
///
/// Uses the v2 OpenAI-compatible endpoint at `qianfan.baidubce.com/v2`, which
/// accepts a static bearer API key (format `bce-v3/ALTAK-...`). The legacy v1
/// `wenxinworkshop` endpoint required AK/SK + OAuth `access_token`; the v2 path
/// is the recommended surface and aligns with the `OpenAICompat` protocol.
/// Primary env var is `QIANFAN_API_KEY`; `QIANFAN_AK`/`QIANFAN_SK` are retained
/// as aliases for users still on the AK/SK flow who mint their own token.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "baidu",
    display_name: "Baidu ERNIE",
    default_base_url: "https://qianfan.baidubce.com/v2",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Wired,
    env_vars: &["QIANFAN_API_KEY", "QIANFAN_AK", "QIANFAN_SK"],
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
    // ERNIE 4.5 series — current flagship (GA March 2025). Multimodal.
    ModelDef {
        id: "ernie-4.5-turbo-128k",
        provider_id: "baidu",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "ernie-4.5-turbo-32k",
        provider_id: "baidu",
        context_window: 32_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "ernie-4.5-8k",
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
    // ERNIE X1 series — reasoning models (competitor to DeepSeek-R1 / o3-mini).
    ModelDef {
        id: "ernie-x1-turbo-32k",
        provider_id: "baidu",
        context_window: 32_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "ernie-x1-32k",
        provider_id: "baidu",
        context_window: 32_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // ERNIE 4.0 series
    ModelDef {
        id: "ernie-4.0-turbo-128k",
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
    // ERNIE 3.5 series
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
    // ERNIE Speed series — low-latency general-purpose.
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
    // ERNIE Lite / Tiny — cost-optimised.
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
    // ERNIE Character — role-play / persona variant.
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
    // Embeddings.
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
    ModelDef {
        id: "tao-8k",
        provider_id: "baidu",
        context_window: 8_192,
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
