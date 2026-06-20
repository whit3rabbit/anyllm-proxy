use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "gmi_cloud",
    display_name: "GMI Cloud",
    // OpenAI-compatible inference API. Docs: https://docs.gmicloud.ai/
    default_base_url: "https://api.gmi-serving.com/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["GMI_CLOUD_API_KEY"],
    litellm_prefix: "gmi_cloud/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        tool_choice: false,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

// Core GA serverless LLMs on GMI Cloud's Model-as-a-Service catalog. IDs use the
// HuggingFace-style namespace shown in the LLM API reference examples and blog posts.
// Live enumeration available via `GET /v1/models`; refresh as the catalog evolves.
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "deepseek-ai/DeepSeek-R1",
        provider_id: "gmi_cloud",
        context_window: 131_072,
        max_output_tokens: 32_768,
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
        id: "deepseek-ai/DeepSeek-V3",
        provider_id: "gmi_cloud",
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
        id: "deepseek-ai/DeepSeek-R1-Distill-Llama-70B",
        provider_id: "gmi_cloud",
        context_window: 131_072,
        max_output_tokens: 16_384,
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
        id: "deepseek-ai/DeepSeek-R1-Distill-Qwen-32B",
        provider_id: "gmi_cloud",
        context_window: 131_072,
        max_output_tokens: 16_384,
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
        id: "meta-llama/Llama-3.3-70B-Instruct",
        provider_id: "gmi_cloud",
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
        id: "meta-llama/Llama-3.1-8B-Instruct",
        provider_id: "gmi_cloud",
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
        id: "Qwen/Qwen3-235B-A22B-Instruct-2507-FP8",
        provider_id: "gmi_cloud",
        context_window: 262_144,
        max_output_tokens: 16_384,
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
        id: "Qwen/Qwen3-32B-FP8",
        provider_id: "gmi_cloud",
        context_window: 131_072,
        max_output_tokens: 16_384,
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
