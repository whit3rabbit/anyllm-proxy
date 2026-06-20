use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

// Nscale serverless inference. OpenAI-compatible REST API.
// Docs: https://docs.nscale.com (chat completions at POST /v1/chat/completions).
// Auth: Authorization: Bearer $NSCALE_API_KEY.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "nscale",
    display_name: "Nscale",
    default_base_url: "https://inference.api.nscale.com/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["NSCALE_API_KEY"],
    litellm_prefix: "nscale/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        tool_choice: false,
        embeddings: false,
        vision: true,
        batch: false,
    },
};

// GA chat/completion models. Image-generation SKUs (FLUX.1-schnell, SDXL) are
// excluded; this catalog only tracks text LLMs. Context windows reflect the
// upstream HF model cards; Nscale may cap lower on shared endpoints.
pub const MODELS: &[ModelDef] = &[
    // Meta Llama family.
    ModelDef {
        id: "meta-llama/Llama-3.1-8B-Instruct",
        provider_id: "nscale",
        context_window: 128_000,
        max_output_tokens: 4_096,
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
        id: "meta-llama/Llama-3.3-70B-Instruct",
        provider_id: "nscale",
        context_window: 128_000,
        max_output_tokens: 4_096,
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
        id: "meta-llama/Llama-4-Scout-17B-16E-Instruct",
        provider_id: "nscale",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Qwen family.
    ModelDef {
        id: "Qwen/QwQ-32B",
        provider_id: "nscale",
        context_window: 32_768,
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
        id: "Qwen/Qwen2.5-Coder-3B-Instruct",
        provider_id: "nscale",
        context_window: 32_768,
        max_output_tokens: 4_096,
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
        id: "Qwen/Qwen2.5-Coder-7B-Instruct",
        provider_id: "nscale",
        context_window: 32_768,
        max_output_tokens: 4_096,
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
        id: "Qwen/Qwen2.5-Coder-32B-Instruct",
        provider_id: "nscale",
        context_window: 32_768,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // DeepSeek R1 distilled reasoning models.
    ModelDef {
        id: "deepseek-ai/DeepSeek-R1-Distill-Qwen-1.5B",
        provider_id: "nscale",
        context_window: 128_000,
        max_output_tokens: 4_096,
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
        id: "deepseek-ai/DeepSeek-R1-Distill-Qwen-7B",
        provider_id: "nscale",
        context_window: 128_000,
        max_output_tokens: 4_096,
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
        id: "deepseek-ai/DeepSeek-R1-Distill-Qwen-14B",
        provider_id: "nscale",
        context_window: 128_000,
        max_output_tokens: 4_096,
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
        provider_id: "nscale",
        context_window: 128_000,
        max_output_tokens: 4_096,
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
        id: "deepseek-ai/DeepSeek-R1-Distill-Llama-8B",
        provider_id: "nscale",
        context_window: 128_000,
        max_output_tokens: 4_096,
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
        id: "deepseek-ai/DeepSeek-R1-Distill-Llama-70B",
        provider_id: "nscale",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // Mistral family.
    ModelDef {
        id: "mistralai/mixtral-8x22b-instruct-v0.1",
        provider_id: "nscale",
        context_window: 65_536,
        max_output_tokens: 4_096,
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
