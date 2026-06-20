use crate::model::{ModelCapabilities, ModelDef, ModelStatus};
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

pub const PROVIDER: ProviderDef = ProviderDef {
    id: "databricks",
    display_name: "Databricks",
    // Workspace-scoped: https://<workspace>.cloud.databricks.com/serving-endpoints
    // OpenAI-compat chat lives at /serving-endpoints/{endpoint}/invocations
    // or /serving-endpoints/v1/chat/completions. Left empty: users must set
    // DATABRICKS_HOST (or the proxy base URL) per workspace.
    default_base_url: "",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::Bearer,
    status: ProviderStatus::Stub,
    // DATABRICKS_TOKEN is the canonical env var; keep DATABRICKS_API_KEY as a
    // proxy-specific alias. DATABRICKS_HOST carries the workspace URL.
    env_vars: &["DATABRICKS_TOKEN", "DATABRICKS_API_KEY", "DATABRICKS_HOST"],
    litellm_prefix: "databricks/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        tool_use: true,
        tool_choice: false,
        embeddings: true,
        vision: true,
        batch: false,
    },
};

// GA pay-per-token Foundation Model API endpoints, as documented at
// docs.databricks.com/aws/en/machine-learning/foundation-model-apis/supported-models.
// Model IDs match the serving endpoint names (used directly in the OpenAI-compat
// URL path). Preview / coding-specific / provisioned-throughput-only models are
// intentionally excluded. Retired endpoints (dbrx-instruct, mixtral-8x7b-instruct,
// llama-3.1-70b, llama-3.1-405b for pay-per-token) are also excluded.
pub const MODELS: &[ModelDef] = &[
    // --- Meta Llama (text chat) ---
    ModelDef {
        id: "databricks-meta-llama-3-3-70b-instruct",
        provider_id: "databricks",
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
        id: "databricks-meta-llama-3-1-8b-instruct",
        provider_id: "databricks",
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
    // --- Anthropic Claude (hosted on Databricks, pay-per-token) ---
    ModelDef {
        id: "databricks-claude-sonnet-4-5",
        provider_id: "databricks",
        context_window: 200_000,
        max_output_tokens: 64_000,
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
        id: "databricks-claude-opus-4-1",
        provider_id: "databricks",
        context_window: 200_000,
        max_output_tokens: 32_000,
        capabilities: ModelCapabilities {
            streaming: true,
            tool_use: true,
            tool_choice: false,
            vision: true,
            extended_thinking: true,
        },
        status: ModelStatus::Available,
    },
    // --- Embeddings ---
    ModelDef {
        id: "databricks-gte-large-en",
        provider_id: "databricks",
        context_window: 8_192,
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
        id: "databricks-bge-large-en",
        provider_id: "databricks",
        context_window: 512,
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
