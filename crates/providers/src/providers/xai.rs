use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "xai",
    display_name: "xAI",
    // Official REST base URL. OpenAI-compatible Chat Completions at `/v1/chat/completions`.
    default_base_url: "https://api.x.ai/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["XAI_API_KEY"],
    litellm_prefix: "xai/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        tool_choice: false,
        // xAI does not publish a public embeddings endpoint as of this writing.
        embeddings: false,
        // Vision is supported on a subset of models (grok-2-vision-1212, grok-4.1-fast, etc.).
        vision: true,
        batch: false,
    },
};

// Context windows and max output tokens verified against LiteLLM's
// `model_prices_and_context_window.json` (xai/ provider entries) and xAI docs.
// Only publicly GA models are listed here.
pub const MODELS: &[ModelDef] = &[
    // Grok 4 (flagship reasoning). `grok-4` is an alias for the latest stable release.
    // 256K context / 256K max output, always-on reasoning, tool calling, no image input.
    ModelDef {
        id: "grok-4",
        provider_id: "xai",
        context_window: 256_000,
        max_output_tokens: 256_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // Grok 4 Fast - two variants exposed as separate model IDs by the xAI API.
    // 2M token context window; reasoning variant has always-on thinking.
    ModelDef {
        id: "grok-4-fast-reasoning",
        provider_id: "xai",
        context_window: 2_000_000,
        max_output_tokens: 2_000_000,
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
        id: "grok-4-fast-non-reasoning",
        provider_id: "xai",
        context_window: 2_000_000,
        max_output_tokens: 2_000_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Grok 3 family. 131,072 context window. Standard variant has no reasoning;
    // mini variant supports reasoning via the `reasoning_effort` parameter.
    ModelDef {
        id: "grok-3",
        provider_id: "xai",
        context_window: 131_072,
        max_output_tokens: 131_072,
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
        id: "grok-3-mini",
        provider_id: "xai",
        context_window: 131_072,
        max_output_tokens: 131_072,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // Grok 2 dated snapshots. Vision variant accepts image input; text variant does not.
    ModelDef {
        id: "grok-2-vision-1212",
        provider_id: "xai",
        context_window: 32_768,
        max_output_tokens: 32_768,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "grok-2-1212",
        provider_id: "xai",
        context_window: 131_072,
        max_output_tokens: 131_072,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Grok Code Fast 1 - agentic coding model. 256K context.
    ModelDef {
        id: "grok-code-fast-1",
        provider_id: "xai",
        context_window: 256_000,
        max_output_tokens: 256_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
];
