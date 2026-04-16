use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

// Nebius AI Studio (rebranded to "Nebius Token Factory" in 2025). The public
// OpenAI-compatible endpoint moved from api.studio.nebius.ai to
// api.tokenfactory.nebius.com. Docs: https://docs.tokenfactory.nebius.com/
// litellm_prefix intentionally kept as "nebius/" for catalog compatibility.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "nebius",
    display_name: "Nebius AI Studio",
    default_base_url: "https://api.tokenfactory.nebius.com/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["NEBIUS_API_KEY"],
    litellm_prefix: "nebius/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: true,
        vision: true,
        batch: false,
    },
};

// Model list sourced from LiteLLM's model_prices_and_context_window.json
// (nebius/* entries) cross-checked against docs.tokenfactory.nebius.com.
// Context/output token figures mirror LiteLLM's published values; some
// providers cap max_output below the full context in practice.
pub const MODELS: &[ModelDef] = &[
    // --- Meta Llama ---
    ModelDef {
        id: "meta-llama/Meta-Llama-3.1-8B-Instruct",
        provider_id: "nebius",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "meta-llama/Meta-Llama-3.1-70B-Instruct",
        provider_id: "nebius",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "meta-llama/Meta-Llama-3.1-405B-Instruct",
        provider_id: "nebius",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "meta-llama/Llama-3.3-70B-Instruct",
        provider_id: "nebius",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "meta-llama/Llama-Guard-3-8B",
        provider_id: "nebius",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // --- DeepSeek ---
    ModelDef {
        id: "deepseek-ai/DeepSeek-R1",
        provider_id: "nebius",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "deepseek-ai/DeepSeek-R1-0528",
        provider_id: "nebius",
        context_window: 164_000,
        max_output_tokens: 164_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "deepseek-ai/DeepSeek-R1-Distill-Llama-70B",
        provider_id: "nebius",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "deepseek-ai/DeepSeek-V3",
        provider_id: "nebius",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "deepseek-ai/DeepSeek-V3-0324",
        provider_id: "nebius",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // --- Qwen (Alibaba) ---
    ModelDef {
        id: "Qwen/Qwen3-4B",
        provider_id: "nebius",
        context_window: 32_768,
        max_output_tokens: 32_768,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Qwen/Qwen3-14B",
        provider_id: "nebius",
        context_window: 32_768,
        max_output_tokens: 32_768,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Qwen/Qwen3-30B-A3B",
        provider_id: "nebius",
        context_window: 32_768,
        max_output_tokens: 32_768,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Qwen/Qwen3-32B",
        provider_id: "nebius",
        context_window: 32_768,
        max_output_tokens: 32_768,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Qwen/Qwen3-235B-A22B",
        provider_id: "nebius",
        context_window: 262_144,
        max_output_tokens: 262_144,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Qwen/QwQ-32B",
        provider_id: "nebius",
        context_window: 32_768,
        max_output_tokens: 32_768,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Qwen/Qwen2.5-32B-Instruct",
        provider_id: "nebius",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Qwen/Qwen2.5-72B-Instruct",
        provider_id: "nebius",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Qwen/Qwen2.5-Coder-7B",
        provider_id: "nebius",
        context_window: 32_768,
        max_output_tokens: 32_768,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Qwen/Qwen2-VL-7B-Instruct",
        provider_id: "nebius",
        context_window: 131_072,
        max_output_tokens: 131_072,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Qwen/Qwen2-VL-72B-Instruct",
        provider_id: "nebius",
        context_window: 131_072,
        max_output_tokens: 131_072,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "Qwen/Qwen2.5-VL-72B-Instruct",
        provider_id: "nebius",
        context_window: 131_072,
        max_output_tokens: 131_072,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // --- Mistral ---
    ModelDef {
        id: "mistralai/Mistral-Nemo-Instruct-2407",
        provider_id: "nebius",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // --- Google Gemma ---
    ModelDef {
        id: "google/gemma-3-27b-it",
        provider_id: "nebius",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // --- NVIDIA Nemotron ---
    ModelDef {
        id: "nvidia/Llama-3.3-Nemotron-Super-49B-v1",
        provider_id: "nebius",
        context_window: 131_072,
        max_output_tokens: 131_072,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "nvidia/Llama-3.1-Nemotron-Ultra-253B-v1",
        provider_id: "nebius",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // --- NousResearch Hermes ---
    ModelDef {
        id: "NousResearch/Hermes-3-Llama-3.1-405B",
        provider_id: "nebius",
        context_window: 128_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // --- Embedding models ---
    ModelDef {
        id: "BAAI/bge-en-icl",
        provider_id: "nebius",
        context_window: 32_768,
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
        id: "BAAI/bge-multilingual-gemma2",
        provider_id: "nebius",
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
    ModelDef {
        id: "intfloat/e5-mistral-7b-instruct",
        provider_id: "nebius",
        context_window: 32_768,
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
