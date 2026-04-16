use crate::model::ModelDef;
use crate::provider::{
    AuthKind, ProviderCapabilities, ProviderDef, ProviderProtocol, ProviderStatus,
};

/// llamafile — Mozilla Ocho / Justine Tunney single-file LLM distribution.
///
/// Packages a llama.cpp server plus a model weights file into one Cosmopolitan
/// Libc executable that runs across OS/CPU combinations. When launched it hosts
/// llama.cpp's HTTP server, which exposes OpenAI-compatible endpoints at
/// `http://localhost:8080/v1` (`/v1/chat/completions`, `/v1/completions`,
/// `/v1/embeddings`). No authentication is required by default; the
/// `Authorization` header is ignored unless `--api-key` is passed at launch.
///
/// Since each llamafile bundles a single model, no fixed catalog ships here;
/// the `model` field in requests is effectively ignored by the server.
pub const PROVIDER: ProviderDef = ProviderDef {
    id: "llamafile",
    display_name: "llamafile",
    default_base_url: "http://localhost:8080/v1",
    protocol: ProviderProtocol::OpenAICompat,
    auth: AuthKind::None,
    status: ProviderStatus::Stub,
    env_vars: &[],
    litellm_prefix: "llamafile/",
    capabilities: ProviderCapabilities {
        chat_completions: true,
        streaming: true,
        // llama.cpp server supports function/tool calling for compatible
        // instruction-tuned models (grammar-constrained JSON output).
        tool_use: true,
        // `/v1/embeddings` available when the bundled model supports it
        // (or when launched with `--embedding`).
        embeddings: true,
        // Vision works with multimodal llamafiles (LLaVA family) that ship
        // an mmproj file; text-only llamafiles reject image inputs.
        vision: true,
        batch: false,
    },
};

// Single-file bundle: the model is baked into the executable, no catalog.
pub const MODELS: &[ModelDef] = &[];
