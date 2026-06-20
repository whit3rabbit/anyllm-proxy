use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Volcano Engine (ByteDance Ark) — OpenAI-compatible endpoint.
///
/// Ark exposes an OpenAI-compatible Chat Completions API under
/// `https://ark.cn-beijing.volces.com/api/v3`. Auth is `Authorization: Bearer <key>`.
/// Canonical env var is `ARK_API_KEY` (as used by the official `volcenginesdkarkruntime`
/// Python SDK); `VOLCENGINE_API_KEY` is accepted as an alias by many third-party tools.
///
/// Models can be referenced either by model ID (e.g. `doubao-seed-2-0-pro-260215`)
/// after activation in the Ark console, or by endpoint ID (e.g. `ep-YYYYMMDDHHMMSS-xxxxx`).
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "volcengine",
    display_name: "Volcano Engine",
    default_base_url: "https://ark.cn-beijing.volces.com/api/v3",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    // ARK_API_KEY is the canonical name (official SDK). VOLCENGINE_API_KEY is a
    // common alias used by third-party integrations (LobeHub, MCP servers).
    env_vars: &["ARK_API_KEY", "VOLCENGINE_API_KEY"],
    litellm_prefix: "volcengine/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        tool_choice: false,
        // Ark offers Doubao embedding models (doubao-embedding-*) via the same endpoint.
        embeddings: true,
        vision: true,
        batch: false,
    },
};

/// Doubao Seed 2.0 family — verified against LiteLLM's
/// `model_prices_and_context_window.json` (`litellm_provider: volcengine`).
/// Context window 256k input, 128k max output, tools + vision + reasoning.
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        id: "doubao-seed-2-0-pro-260215",
        provider_id: "volcengine",
        context_window: 256_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "doubao-seed-2-0-lite-260215",
        provider_id: "volcengine",
        context_window: 256_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "doubao-seed-2-0-mini-260215",
        provider_id: "volcengine",
        context_window: 256_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "doubao-seed-2-0-code-preview-260215",
        provider_id: "volcengine",
        context_window: 256_000,
        max_output_tokens: 128_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // Doubao embedding models. max_input_tokens 4096 per LiteLLM; no output tokens.
    ModelDef {
        id: "doubao-embedding-large-text-250515",
        provider_id: "volcengine",
        context_window: 4_096,
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
        id: "doubao-embedding-large-text-240915",
        provider_id: "volcengine",
        context_window: 4_096,
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
        id: "doubao-embedding-text-240715",
        provider_id: "volcengine",
        context_window: 4_096,
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
