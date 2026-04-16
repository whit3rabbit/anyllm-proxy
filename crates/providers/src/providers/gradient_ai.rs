use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

// DigitalOcean Gradient AI Platform (formerly marketed as "Gradient AI").
// Note: the original Gradient.ai (Boston) was discontinued after acquisition;
// this id now tracks DigitalOcean's Gradient AI serverless inference surface.
//
// Base URL and auth per docs.digitalocean.com/products/gradient-ai-platform:
//   POST https://inference.do-ai.run/v1/chat/completions
//   Authorization: Bearer $MODEL_ACCESS_KEY
// OpenAI-compatible chat completions with streaming. Tool calling is supported
// on hosted Anthropic/OpenAI/Llama variants; embeddings on this endpoint are
// not publicly documented (embedding models exist only for knowledge-base use).
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "gradient_ai",
    display_name: "DigitalOcean Gradient AI",
    default_base_url: "https://inference.do-ai.run/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["DIGITALOCEAN_INFERENCE_KEY", "GRADIENT_ACCESS_TOKEN"],
    litellm_prefix: "gradient_ai/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

// Only DO-hosted GA serverless models are listed. Anthropic/OpenAI models
// exposed via Gradient agents use the same endpoint but are tracked under
// their own provider ids; context windows for those vary by upstream.
pub const MODELS: &[ModelDef] = &[
    // Meta Llama 3.3 70B Instruct, hosted by DigitalOcean. 128K context.
    ModelDef {
        id: "llama3.3-70b-instruct",
        provider_id: "gradient_ai",
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
    // DeepSeek R1 Distill (Llama 70B base). Reasoning model; no function calling.
    ModelDef {
        id: "deepseek-r1-distill-llama-70b",
        provider_id: "gradient_ai",
        context_window: 128_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
];
