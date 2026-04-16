use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// OVHCloud AI Endpoints — OpenAI-compatible gateway hosted on OVHcloud Public Cloud.
///
/// Base URL is the unified gateway documented by OVHcloud and the Apache Airflow
/// provider: `https://oai.endpoints.kepler.ai.cloud.ovh.net/v1`. Per-model URLs
/// of the form `https://<slug>.endpoints.kepler.ai.cloud.ovh.net/api/openai_compat/v1`
/// also exist; callers can override via `api_base`.
///
/// Auth: `Authorization: Bearer <token>` using an OVHcloud Manager-issued API key
/// (`OVH_AI_ENDPOINTS_ACCESS_TOKEN`). LiteLLM uses the alias `OVHCLOUD_API_KEY`.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "ovhcloud",
    display_name: "OVHCloud AI Endpoints",
    default_base_url: "https://oai.endpoints.kepler.ai.cloud.ovh.net/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    env_vars: &["OVH_AI_ENDPOINTS_ACCESS_TOKEN", "OVHCLOUD_API_KEY"],
    litellm_prefix: "ovhcloud/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        embeddings: true,
        vision: true,
        batch: false,
    },
};

/// GA models listed in the OVHcloud AI Endpoints catalog
/// (https://endpoints.ai.cloud.ovh.net/catalog). Model IDs match the slugs
/// documented by LiteLLM and the OVHcloud catalog (underscore-separated
/// version numbers where OVHcloud uses them, e.g. `Meta-Llama-3_3-70B-Instruct`).
///
/// `max_output_tokens` is left conservative (8k) because OVHcloud does not
/// publish a hard per-request cap separate from the context window — callers
/// should set `max_tokens` explicitly.
pub const MODELS: &[ModelDef] = &[
    // Meta Llama 3.3 70B Instruct — 131k context, function calling.
    ModelDef {
        id: "Meta-Llama-3_3-70B-Instruct",
        provider_id: "ovhcloud",
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
    // Mistral 7B Instruct v0.3 — 127k context.
    ModelDef {
        id: "Mistral-7B-Instruct-v0.3",
        provider_id: "ovhcloud",
        context_window: 127_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Mistral Nemo Instruct 2407 — 118k context.
    ModelDef {
        id: "Mistral-Nemo-Instruct-2407",
        provider_id: "ovhcloud",
        context_window: 118_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // Mistral Small 3.2 24B Instruct (2506) — 128k context, vision + tools.
    ModelDef {
        id: "Mistral-Small-3.2-24B-Instruct-2506",
        provider_id: "ovhcloud",
        context_window: 128_000,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: true,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // OpenAI gpt-oss 120B reasoning model — 131k context.
    ModelDef {
        id: "gpt-oss-120b",
        provider_id: "ovhcloud",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // OpenAI gpt-oss 20B reasoning model — 131k context.
    ModelDef {
        id: "gpt-oss-20b",
        provider_id: "ovhcloud",
        context_window: 131_072,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // Qwen3 32B — 32k context, tool use, reasoning-capable.
    ModelDef {
        id: "Qwen3-32B",
        provider_id: "ovhcloud",
        context_window: 32_768,
        max_output_tokens: 8_192,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            vision: false,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // BGE-M3 embedding model — 8k context, no chat/tool support.
    ModelDef {
        id: "BGE-M3",
        provider_id: "ovhcloud",
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
