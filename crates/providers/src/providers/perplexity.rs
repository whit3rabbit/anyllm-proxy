use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "perplexity",
    display_name: "Perplexity AI",
    default_base_url: "https://api.perplexity.ai",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["PERPLEXITYAI_API_KEY", "PERPLEXITY_API_KEY"],
    litellm_prefix: "perplexity/",
    // Sonar chat-completions is OpenAI-compatible: Bearer auth, /chat/completions,
    // SSE streaming, image_url content blocks. Function/tool calling is only exposed
    // through the Pro Search preset (Responses API), not the GA Sonar chat endpoint,
    // so tool_use stays false at the provider level. No embeddings or batch API.
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: false,
        embeddings: false,
        vision: true,
        batch: false,
    },
};

// Publicly GA Sonar catalog (docs.perplexity.ai/getting-started/models and
// LiteLLM model_prices_and_context_window.json). Context windows per LiteLLM;
// max_output_tokens left at 0 when Perplexity does not publish a hard cap.
// r1-1776 is intentionally omitted: it was retired from the GA catalog.
pub const MODELS: &[ModelDef] = &[
    // Sonar: lightweight, grounded search. 128k context.
    ModelDef {
        id: "sonar",
        provider_id: "perplexity",
        context_window: 128_000,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Sonar Pro: advanced grounded search, larger context and capped output.
    ModelDef {
        id: "sonar-pro",
        provider_id: "perplexity",
        context_window: 200_000,
        max_output_tokens: 8_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Sonar Reasoning: Chain-of-Thought reasoning model.
    ModelDef {
        id: "sonar-reasoning",
        provider_id: "perplexity",
        context_window: 128_000,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // Sonar Reasoning Pro: premium CoT reasoning tier.
    ModelDef {
        id: "sonar-reasoning-pro",
        provider_id: "perplexity",
        context_window: 128_000,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // Sonar Deep Research: exhaustive multi-step research agent.
    ModelDef {
        id: "sonar-deep-research",
        provider_id: "perplexity",
        context_window: 128_000,
        max_output_tokens: 0,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
];
