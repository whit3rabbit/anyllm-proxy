use crate::model::ModelDef;
use crate::provider::{ProviderDef, ProviderProtocol};
use crate::providers;

/// All registered providers. Add new providers here and in `crates/providers/src/providers/`.
static ALL_PROVIDERS: &[&ProviderDef] = &[
    // Implemented / wired
    &providers::openai::PROVIDER,
    &providers::anthropic::PROVIDER,
    &providers::gemini::PROVIDER,
    &providers::vertex::PROVIDER,
    &providers::azure::PROVIDER,
    &providers::bedrock::PROVIDER,
    // Stubs (OpenAI-compatible)
    &providers::groq::PROVIDER,
    &providers::together::PROVIDER,
    &providers::openrouter::PROVIDER,
    &providers::fireworks::PROVIDER,
    &providers::mistral::PROVIDER,
    &providers::perplexity::PROVIDER,
    &providers::deepseek::PROVIDER,
    &providers::cerebras::PROVIDER,
    &providers::ollama::PROVIDER,
    &providers::vllm::PROVIDER,
    &providers::databricks::PROVIDER,
    &providers::sambanova::PROVIDER,
    &providers::nebius::PROVIDER,
    &providers::deepinfra::PROVIDER,
    &providers::novita::PROVIDER,
    &providers::cohere::PROVIDER,
    &providers::ai21::PROVIDER,
    &providers::huggingface::PROVIDER,
    &providers::anyscale::PROVIDER,
    // New stubs (LiteLLM parity)
    &providers::xai::PROVIDER,
    &providers::nvidia_nim::PROVIDER,
    &providers::codestral::PROVIDER,
    &providers::moonshot::PROVIDER,
    &providers::volcengine::PROVIDER,
    &providers::minimax::PROVIDER,
    &providers::zhipuai::PROVIDER,
    &providers::featherless::PROVIDER,
    &providers::friendliai::PROVIDER,
    &providers::lambda::PROVIDER,
    &providers::hyperbolic::PROVIDER,
    &providers::nscale::PROVIDER,
    &providers::github::PROVIDER,
    &providers::aleph_alpha::PROVIDER,
    &providers::nlp_cloud::PROVIDER,
    &providers::clarifai::PROVIDER,
    &providers::predibase::PROVIDER,
    &providers::replicate::PROVIDER,
    &providers::chutes::PROVIDER,
    &providers::gmi_cloud::PROVIDER,
    &providers::meta_llama::PROVIDER,
    &providers::ai_ml_api::PROVIDER,
    &providers::voyage::PROVIDER,
    &providers::scaleway::PROVIDER,
    &providers::baseten::PROVIDER,
    &providers::lm_studio::PROVIDER,
    &providers::llamafile::PROVIDER,
    &providers::xinference::PROVIDER,
    &providers::azure_ai::PROVIDER,
    &providers::watsonx::PROVIDER,
    &providers::cloudflare::PROVIDER,
    &providers::snowflake::PROVIDER,
    &providers::sagemaker::PROVIDER,
    &providers::petals::PROVIDER,
    &providers::triton::PROVIDER,
    // New stubs (round 2)
    &providers::dashscope::PROVIDER,
    &providers::jina::PROVIDER,
    &providers::ovhcloud::PROVIDER,
    &providers::infinity::PROVIDER,
    &providers::gradient_ai::PROVIDER,
    &providers::galadriel::PROVIDER,
    &providers::morph::PROVIDER,
    &providers::lemonade::PROVIDER,
    &providers::docker_model_runner::PROVIDER,
    &providers::xiaomi_mimo::PROVIDER,
    &providers::public_ai::PROVIDER,
    &providers::nanogpt::PROVIDER,
    &providers::wandb::PROVIDER,
    &providers::bytez::PROVIDER,
    // New stubs (OmniRoute parity)
    &providers::siliconflow::PROVIDER,
    &providers::blackboxai::PROVIDER,
    &providers::pollinations::PROVIDER,
    &providers::stability::PROVIDER,
    &providers::iflytek::PROVIDER,
    &providers::baidu::PROVIDER,
    &providers::lmsys::PROVIDER,
    &providers::deepgram::PROVIDER,
    &providers::assemblyai::PROVIDER,
    &providers::elevenlabs::PROVIDER,
    &providers::playht::PROVIDER,
    &providers::cartesia::PROVIDER,
    &providers::brave::PROVIDER,
    &providers::serper::PROVIDER,
    &providers::tavily::PROVIDER,
    &providers::exa::PROVIDER,
];

/// Models keyed by provider id, in the same order as `ALL_PROVIDERS`.
static ALL_MODELS: &[(&str, &[ModelDef])] = &[
    ("openai", providers::openai::MODELS),
    ("anthropic", providers::anthropic::MODELS),
    ("gemini", providers::gemini::MODELS),
    ("vertex_ai", providers::vertex::MODELS),
    ("azure", providers::azure::MODELS),
    ("bedrock", providers::bedrock::MODELS),
    ("groq", providers::groq::MODELS),
    ("together_ai", providers::together::MODELS),
    ("openrouter", providers::openrouter::MODELS),
    ("fireworks_ai", providers::fireworks::MODELS),
    ("mistral", providers::mistral::MODELS),
    ("perplexity", providers::perplexity::MODELS),
    ("deepseek", providers::deepseek::MODELS),
    ("cerebras", providers::cerebras::MODELS),
    ("ollama", providers::ollama::MODELS),
    ("hosted_vllm", providers::vllm::MODELS),
    ("databricks", providers::databricks::MODELS),
    ("sambanova", providers::sambanova::MODELS),
    ("nebius", providers::nebius::MODELS),
    ("deepinfra", providers::deepinfra::MODELS),
    ("novita", providers::novita::MODELS),
    ("cohere_chat", providers::cohere::MODELS),
    ("ai21", providers::ai21::MODELS),
    ("huggingface", providers::huggingface::MODELS),
    ("anyscale", providers::anyscale::MODELS),
    // New stubs (LiteLLM parity)
    ("xai", providers::xai::MODELS),
    ("nvidia_nim", providers::nvidia_nim::MODELS),
    ("codestral", providers::codestral::MODELS),
    ("moonshot", providers::moonshot::MODELS),
    ("volcengine", providers::volcengine::MODELS),
    ("minimax", providers::minimax::MODELS),
    ("zhipuai", providers::zhipuai::MODELS),
    ("featherless_ai", providers::featherless::MODELS),
    ("friendliai", providers::friendliai::MODELS),
    ("lambda_ai", providers::lambda::MODELS),
    ("hyperbolic", providers::hyperbolic::MODELS),
    ("nscale", providers::nscale::MODELS),
    ("github", providers::github::MODELS),
    ("aleph_alpha", providers::aleph_alpha::MODELS),
    ("nlp_cloud", providers::nlp_cloud::MODELS),
    ("clarifai", providers::clarifai::MODELS),
    ("predibase", providers::predibase::MODELS),
    ("replicate", providers::replicate::MODELS),
    ("chutes", providers::chutes::MODELS),
    ("gmi_cloud", providers::gmi_cloud::MODELS),
    ("meta_llama", providers::meta_llama::MODELS),
    ("ai_ml_api", providers::ai_ml_api::MODELS),
    ("voyage", providers::voyage::MODELS),
    ("scaleway", providers::scaleway::MODELS),
    ("baseten", providers::baseten::MODELS),
    ("lm_studio", providers::lm_studio::MODELS),
    ("llamafile", providers::llamafile::MODELS),
    ("xinference", providers::xinference::MODELS),
    ("azure_ai", providers::azure_ai::MODELS),
    ("watsonx", providers::watsonx::MODELS),
    ("cloudflare", providers::cloudflare::MODELS),
    ("snowflake", providers::snowflake::MODELS),
    ("sagemaker", providers::sagemaker::MODELS),
    ("petals", providers::petals::MODELS),
    ("triton", providers::triton::MODELS),
    // New stubs (round 2)
    ("dashscope", providers::dashscope::MODELS),
    ("jina", providers::jina::MODELS),
    ("ovhcloud", providers::ovhcloud::MODELS),
    ("infinity", providers::infinity::MODELS),
    ("gradient_ai", providers::gradient_ai::MODELS),
    ("galadriel", providers::galadriel::MODELS),
    ("morph", providers::morph::MODELS),
    ("lemonade", providers::lemonade::MODELS),
    (
        "docker_model_runner",
        providers::docker_model_runner::MODELS,
    ),
    ("xiaomi_mimo", providers::xiaomi_mimo::MODELS),
    ("public_ai", providers::public_ai::MODELS),
    ("nanogpt", providers::nanogpt::MODELS),
    ("wandb", providers::wandb::MODELS),
    ("bytez", providers::bytez::MODELS),
    // New stubs (OmniRoute parity)
    ("siliconflow", providers::siliconflow::MODELS),
    ("blackboxai", providers::blackboxai::MODELS),
    ("pollinations", providers::pollinations::MODELS),
    ("stability_ai", providers::stability::MODELS),
    ("iflytek", providers::iflytek::MODELS),
    ("baidu", providers::baidu::MODELS),
    ("lmsys", providers::lmsys::MODELS),
    ("deepgram", providers::deepgram::MODELS),
    ("assemblyai", providers::assemblyai::MODELS),
    ("elevenlabs", providers::elevenlabs::MODELS),
    ("playht", providers::playht::MODELS),
    ("cartesia", providers::cartesia::MODELS),
    ("brave", providers::brave::MODELS),
    ("serper", providers::serper::MODELS),
    ("tavily", providers::tavily::MODELS),
    ("exa", providers::exa::MODELS),
];

/// Look up a provider by its `id` field (e.g. `"groq"`, `"together_ai"`).
pub fn get_provider(id: &str) -> Option<&'static ProviderDef> {
    ALL_PROVIDERS.iter().find(|p| p.id == id).copied()
}

/// All registered providers.
pub fn all_providers() -> impl Iterator<Item = &'static ProviderDef> {
    ALL_PROVIDERS.iter().copied()
}

/// All models registered for a given provider id.
pub fn list_models(provider_id: &str) -> &'static [ModelDef] {
    ALL_MODELS
        .iter()
        .find(|(id, _)| *id == provider_id)
        .map(|(_, models)| *models)
        .unwrap_or(&[])
}

/// Look up a specific model by provider id and model id.
pub fn get_model(provider_id: &str, model_id: &str) -> Option<&'static ModelDef> {
    list_models(provider_id).iter().find(|m| m.id == model_id)
}

/// Resolve a provider id to the BackendKind string and a default base URL override.
///
/// Returns `None` if the provider id is not recognized.
/// Returns `Some((backend_kind_str, base_url))` where:
/// - `backend_kind_str` matches the string accepted by `BackendKind` parsing in proxy config
///   (e.g. `"openai"`, `"anthropic"`, `"gemini"`, `"vertex"`, `"azure"`, `"bedrock"`)
/// - `base_url` is the provider's default base URL (may be empty for per-deployment providers)
pub fn resolve_backend(provider_id: &str) -> Option<(&'static str, &'static str)> {
    let p = get_provider(provider_id)?;
    let kind_str = match p.protocol {
        ProviderProtocol::OpenAICompat => "openai",
        ProviderProtocol::AzureOpenAI => "azure",
        ProviderProtocol::VertexAI => "vertex",
        ProviderProtocol::GeminiOpenAI => "gemini",
        ProviderProtocol::GeminiNative => "gemini",
        ProviderProtocol::AnthropicNative => "anthropic",
        ProviderProtocol::BedrockNative => "bedrock",
        ProviderProtocol::Custom => return None,
    };
    Some((kind_str, p.default_base_url))
}

/// Find a provider by its LiteLLM routing prefix (e.g. `"groq/"` or `"together_ai/"`).
/// Used by `parse_provider_model()` in litellm config parsing.
pub fn find_by_litellm_prefix(prefix: &str) -> Option<&'static ProviderDef> {
    ALL_PROVIDERS
        .iter()
        .find(|p| !p.litellm_prefix.is_empty() && prefix == p.litellm_prefix)
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_known_providers_resolve() {
        for provider in all_providers() {
            // Every provider with a non-Custom protocol must resolve to a backend kind.
            if provider.protocol != ProviderProtocol::Custom {
                let result = resolve_backend(provider.id);
                assert!(
                    result.is_some(),
                    "provider '{}' should resolve to a backend kind",
                    provider.id
                );
            }
        }
    }

    #[test]
    fn get_provider_lookup() {
        assert!(get_provider("openai").is_some());
        assert!(get_provider("groq").is_some());
        assert!(get_provider("nonexistent").is_none());
    }

    #[test]
    fn openai_models_populated() {
        assert!(!list_models("openai").is_empty());
    }

    #[test]
    fn anthropic_models_populated() {
        assert!(!list_models("anthropic").is_empty());
    }

    #[test]
    fn self_hosted_models_empty() {
        // Self-hosted providers have no static model list by design.
        assert!(list_models("ollama").is_empty());
        assert!(list_models("hosted_vllm").is_empty());
    }

    #[test]
    fn litellm_prefix_lookup() {
        let p = find_by_litellm_prefix("groq/").unwrap();
        assert_eq!(p.id, "groq");

        let p = find_by_litellm_prefix("together_ai/").unwrap();
        assert_eq!(p.id, "together_ai");

        assert!(find_by_litellm_prefix("unknown/").is_none());
    }

    #[test]
    fn resolve_backend_stub_routes_to_openai() {
        let (kind, _url) = resolve_backend("groq").unwrap();
        assert_eq!(kind, "openai");
    }

    #[test]
    fn resolve_backend_anthropic() {
        let (kind, _) = resolve_backend("anthropic").unwrap();
        assert_eq!(kind, "anthropic");
    }
}
