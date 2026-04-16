use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

// AI/ML API (aimlapi.com): OpenAI-compatible aggregator exposing 400+ models
// across OpenAI, Anthropic, Google, Meta, DeepSeek, Qwen, xAI, Mistral, etc.
// Chat completions endpoint: https://api.aimlapi.com/v1/chat/completions
// Auth: `Authorization: Bearer <key>`.
// Batch: not advertised as a public endpoint (the proxy does not route batch
// through this provider). Embeddings: supported (multiple embedding models).
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "ai_ml_api",
    display_name: "AI/ML API",
    default_base_url: "https://api.aimlapi.com/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["AIML_API_KEY"],
    litellm_prefix: "ai_ml_api/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: true,
        vision: true,
        batch: false,
    },
};

// Representative GA model subset. AI/ML API proxies upstream providers, so
// context/output window values mirror each upstream's published limits.
// Model IDs match the aimlapi model database; some entries are exposed both
// under short aliases and vendor-prefixed forms (e.g. `gpt-4o` vs
// `openai/gpt-4o`). The short forms are used here for parity with `openai.rs`.
pub const MODELS: &[ModelDef] = &[
    // --- OpenAI ---
    ModelDef {
        id: "gpt-4o",
        provider_id: "ai_ml_api",
        context_window: 128_000,
        max_output_tokens: 16_384,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "gpt-4o-mini",
        provider_id: "ai_ml_api",
        context_window: 128_000,
        max_output_tokens: 16_384,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "gpt-4-turbo",
        provider_id: "ai_ml_api",
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
        id: "gpt-4",
        provider_id: "ai_ml_api",
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
    ModelDef {
        id: "gpt-3.5-turbo",
        provider_id: "ai_ml_api",
        context_window: 16_385,
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
        id: "o1",
        provider_id: "ai_ml_api",
        context_window: 200_000,
        max_output_tokens: 100_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "o3-mini",
        provider_id: "ai_ml_api",
        context_window: 200_000,
        max_output_tokens: 100_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // --- Anthropic ---
    ModelDef {
        id: "claude-3-haiku-20240307",
        provider_id: "ai_ml_api",
        context_window: 200_000,
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
        id: "anthropic/claude-opus-4",
        provider_id: "ai_ml_api",
        context_window: 200_000,
        max_output_tokens: 32_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "anthropic/claude-opus-4.1",
        provider_id: "ai_ml_api",
        context_window: 200_000,
        max_output_tokens: 32_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "anthropic/claude-sonnet-4",
        provider_id: "ai_ml_api",
        context_window: 200_000,
        max_output_tokens: 64_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "claude-sonnet-4-5-20250929",
        provider_id: "ai_ml_api",
        context_window: 200_000,
        max_output_tokens: 64_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "anthropic/claude-haiku-4.5",
        provider_id: "ai_ml_api",
        context_window: 200_000,
        max_output_tokens: 64_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // --- Google Gemini ---
    ModelDef {
        id: "gemini-2.0-flash",
        provider_id: "ai_ml_api",
        context_window: 1_048_576,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "google/gemini-2.5-flash",
        provider_id: "ai_ml_api",
        context_window: 1_048_576,
        max_output_tokens: 65_536,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "google/gemini-2.5-pro",
        provider_id: "ai_ml_api",
        context_window: 1_048_576,
        max_output_tokens: 65_536,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // --- Meta Llama ---
    ModelDef {
        id: "meta-llama/Llama-3.3-70B-Instruct-Turbo",
        provider_id: "ai_ml_api",
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
    // --- Mistral ---
    ModelDef {
        id: "mistralai/Mixtral-8x7B-Instruct-v0.1",
        provider_id: "ai_ml_api",
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
        id: "mistralai/mistral-nemo",
        provider_id: "ai_ml_api",
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
    // --- DeepSeek ---
    ModelDef {
        id: "deepseek-chat",
        provider_id: "ai_ml_api",
        context_window: 128_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "deepseek-reasoner",
        provider_id: "ai_ml_api",
        context_window: 128_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // --- Alibaba Qwen ---
    ModelDef {
        id: "qwen-max",
        provider_id: "ai_ml_api",
        context_window: 32_768,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "qwen-plus",
        provider_id: "ai_ml_api",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "qwen-turbo",
        provider_id: "ai_ml_api",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // --- xAI Grok ---
    ModelDef {
        id: "x-ai/grok-3-beta",
        provider_id: "ai_ml_api",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "x-ai/grok-3-mini-beta",
        provider_id: "ai_ml_api",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "x-ai/grok-4-07-09",
        provider_id: "ai_ml_api",
        context_window: 256_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
];
