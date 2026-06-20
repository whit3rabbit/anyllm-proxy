use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// Snowflake Cortex AI — managed LLM inference inside Snowflake.
///
/// The REST endpoint is per-account and not a single global host:
///   `https://<account-identifier>.snowflakecomputing.com/api/v2/cortex/inference:complete`
/// The request/response schema follows OpenAI Chat Completions. Auth is a
/// bearer token sourced from a programmatic access token (PAT), a key-pair JWT,
/// or an OAuth token. For that reason `default_base_url` is left empty; users
/// must configure `OPENAI_BASE_URL` (or `api_base` in YAML) to their account URL.
///
/// Docs: https://docs.snowflake.com/en/user-guide/snowflake-cortex/cortex-rest-api
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "snowflake",
    display_name: "Snowflake Cortex",
    default_base_url: "",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    // SNOWFLAKE_JWT is the canonical legacy name in this codebase (see docs/ENV.md).
    // SNOWFLAKE_PAT is Snowflake's newer recommended mechanism; both are bearer tokens.
    // SNOWFLAKE_ACCOUNT_ID supplies the per-account hostname component.
    env_vars: &["SNOWFLAKE_JWT", "SNOWFLAKE_PAT", "SNOWFLAKE_ACCOUNT_ID"],
    litellm_prefix: "snowflake/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        // Cortex Inference follows OpenAI chat/completions and supports tool/function calling
        // on models that themselves support it (Llama 3.1+, Mistral Large 2, Claude via Cortex).
        tool_use: true,
        tool_choice: false,
        embeddings: false,
        vision: false,
        batch: false,
    },
};

// Cortex-hosted chat models verified against the Snowflake COMPLETE function
// and Cortex Inference docs. Context windows reflect the underlying model spec
// (Snowflake has not published reduced per-deployment limits for these).
// Region availability varies; treat this list as a superset.
pub const MODELS: &[ModelDef] = &[
    // Snowflake Arctic — 480B MoE, 17B active. Apache 2.0.
    ModelDef {
        id: "snowflake-arctic",
        provider_id: "snowflake",
        context_window: 4_096,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    // SwiftKV-optimized Snowflake variants of Meta Llama.
    ModelDef {
        id: "snowflake-llama-3.3-70b",
        provider_id: "snowflake",
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
    ModelDef {
        id: "snowflake-llama-3.1-405b",
        provider_id: "snowflake",
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
    // Meta Llama baselines hosted on Cortex.
    ModelDef {
        id: "llama3.3-70b",
        provider_id: "snowflake",
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
    ModelDef {
        id: "llama3.1-70b",
        provider_id: "snowflake",
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
        id: "llama3.1-8b",
        provider_id: "snowflake",
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
    // Mistral family.
    ModelDef {
        id: "mistral-large2",
        provider_id: "snowflake",
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
    ModelDef {
        id: "mixtral-8x7b",
        provider_id: "snowflake",
        context_window: 32_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
    ModelDef {
        id: "mistral-7b",
        provider_id: "snowflake",
        context_window: 32_000,
        max_output_tokens: 4_096,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: false,
            tool_choice: false,
            vision: false,
            extended_thinking: false,
        },
        status: ModelStatus::Available,
    },
];
