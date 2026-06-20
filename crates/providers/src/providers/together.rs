use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "together_ai",
    display_name: "Together AI",
    // OpenAI-compatible chat completions at /v1/chat/completions.
    // `.xyz` is the canonical host (api.together.ai aliases to the same).
    default_base_url: "https://api.together.xyz/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["TOGETHER_API_KEY", "TOGETHERAI_API_KEY"],
    litellm_prefix: "together_ai/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        // Function/tool calling is supported on select models (e.g. Llama-3.1/3.3 Turbo,
        // DeepSeek-V3). Some Turbo variants have known tool-calling quirks.
        tool_use: true,
        tool_choice: false,
        // Embeddings endpoint is available (e.g. intfloat/multilingual-e5-large-instruct).
        embeddings: true,
        // Together hosts multimodal models (Qwen VL family), so vision is supported at
        // the endpoint level even if not every chat model accepts images.
        vision: true,
        // No Batch API endpoint.
        batch: false,
    },
};

// Conservative set limited to models whose exact ID and context window were
// verified against Together's serverless docs or other authoritative sources.
// Prefer adding fewer entries than guessing; the rest are routed through the
// proxy as unknown-but-OpenAI-compatible models regardless.
pub const MODELS: &[ModelDef] = &[
    // Meta Llama 3.3 70B Instruct Turbo. 131,072 ctx per Together serverless docs.
    ModelDef {
        id: "meta-llama/Llama-3.3-70B-Instruct-Turbo",
        provider_id: "together_ai",
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
    // Meta Llama 3.1 70B Instruct Turbo. 128K native Llama 3.1 context.
    ModelDef {
        id: "meta-llama/Meta-Llama-3.1-70B-Instruct-Turbo",
        provider_id: "together_ai",
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
    // Meta Llama 3.1 8B Instruct Turbo. Same 128K Llama 3.1 context.
    ModelDef {
        id: "meta-llama/Meta-Llama-3.1-8B-Instruct-Turbo",
        provider_id: "together_ai",
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
    // DeepSeek-R1 reasoning model. 163,839 ctx per Together serverless docs.
    ModelDef {
        id: "deepseek-ai/DeepSeek-R1",
        provider_id: "together_ai",
        context_window: 163_839,
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
    // DeepSeek-V3 general-purpose MoE. 128K native context.
    ModelDef {
        id: "deepseek-ai/DeepSeek-V3",
        provider_id: "together_ai",
        context_window: 128_000,
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
    // Qwen2.5 72B Instruct Turbo. 32,768 ctx on Together's Turbo (FP8) deployment
    // (base model is 128K; Turbo is reduced).
    ModelDef {
        id: "Qwen/Qwen2.5-72B-Instruct-Turbo",
        provider_id: "together_ai",
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
    // Mixtral 8x7B Instruct v0.1. 32,768 native training context.
    ModelDef {
        id: "mistralai/Mixtral-8x7B-Instruct-v0.1",
        provider_id: "together_ai",
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
];
