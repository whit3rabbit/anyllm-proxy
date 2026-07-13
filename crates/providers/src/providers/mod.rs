//! Supported backend LLM providers and their metadata.
//!
//! Each submodule defines a provider configuration, including its protocol,
//! default endpoints, and supported authentication options.

/// AI21 provider definition.
pub mod ai21;
/// AI/ML API provider definition.
pub mod ai_ml_api;
/// Aleph Alpha provider definition.
pub mod aleph_alpha;
/// Anthropic provider definition.
pub mod anthropic;
/// Anyscale provider definition.
pub mod anyscale;
/// AssemblyAI provider definition.
pub mod assemblyai;
/// Azure provider definition.
pub mod azure;
/// Azure AI provider definition.
pub mod azure_ai;
/// Baidu provider definition.
pub mod baidu;
/// Baseten provider definition.
pub mod baseten;
/// Bedrock provider definition.
pub mod bedrock;
/// BlackboxAI provider definition.
pub mod blackboxai;
/// Brave provider definition.
pub mod brave;
/// Bytez provider definition.
pub mod bytez;
/// Cartesia provider definition.
pub mod cartesia;
/// Cerebras provider definition.
pub mod cerebras;
/// Chutes provider definition.
pub mod chutes;
/// Clarifai provider definition.
pub mod clarifai;
/// Cloudflare provider definition.
pub mod cloudflare;
/// Codestral provider definition.
pub mod codestral;
/// Cohere provider definition.
pub mod cohere;
/// Dashscope provider definition.
pub mod dashscope;
/// Databricks provider definition.
pub mod databricks;
/// Deepgram provider definition.
pub mod deepgram;
/// Deepinfra provider definition.
pub mod deepinfra;
/// Deepseek provider definition.
pub mod deepseek;
/// Docker Model Runner provider definition.
pub mod docker_model_runner;
/// Elevenlabs provider definition.
pub mod elevenlabs;
/// Exa provider definition.
pub mod exa;
/// Featherless provider definition.
pub mod featherless;
/// Fireworks provider definition.
pub mod fireworks;
/// Friendliai provider definition.
pub mod friendliai;
/// Galadriel provider definition.
pub mod galadriel;
/// Gemini provider definition.
pub mod gemini;
/// Github provider definition.
pub mod github;
/// GMI Cloud provider definition.
pub mod gmi_cloud;
/// Gradient AI provider definition.
pub mod gradient_ai;
/// Groq provider definition.
pub mod groq;
/// Hugging Face provider definition.
pub mod huggingface;
/// Hyperbolic provider definition.
pub mod hyperbolic;
/// iFlyTek provider definition.
pub mod iflytek;
/// Infinity provider definition.
pub mod infinity;
/// Jina provider definition.
pub mod jina;
/// Lambda provider definition.
pub mod lambda;
/// Lemonade provider definition.
pub mod lemonade;
/// Generated snapshot of LiteLLM models.
pub mod litellm_snapshot;
/// Llamafile provider definition.
pub mod llamafile;
/// LM Studio provider definition.
pub mod lm_studio;
/// LMSYS provider definition.
pub mod lmsys;
/// Meta Llama provider definition.
pub mod meta_llama;
/// Minimax provider definition.
pub mod minimax;
/// Mistral provider definition.
pub mod mistral;
/// Moonshot provider definition.
pub mod moonshot;
/// Morph provider definition.
pub mod morph;
/// NanoGPT provider definition.
pub mod nanogpt;
/// Nebius provider definition.
pub mod nebius;
/// NLP Cloud provider definition.
pub mod nlp_cloud;
/// Novita provider definition.
pub mod novita;
/// Nscale provider definition.
pub mod nscale;
/// NVIDIA NIM provider definition.
pub mod nvidia_nim;
/// Ollama provider definition.
pub mod ollama;
/// OpenAI provider definition.
pub mod openai;
/// OpenRouter provider definition.
pub mod openrouter;
/// OVHcloud provider definition.
pub mod ovhcloud;
/// Perplexity provider definition.
pub mod perplexity;
/// Petals provider definition.
pub mod petals;
/// PlayHT provider definition.
pub mod playht;
/// Pollinations provider definition.
pub mod pollinations;
/// Predibase provider definition.
pub mod predibase;
/// Public AI provider definition.
pub mod public_ai;
/// Replicate provider definition.
pub mod replicate;
/// SageMaker provider definition.
pub mod sagemaker;
/// SambaNova provider definition.
pub mod sambanova;
/// Serper provider definition.
pub mod serper;
/// SiliconFlow provider definition.
pub mod siliconflow;
/// Snowflake provider definition.
pub mod snowflake;
/// Stability provider definition.
pub mod stability;
/// Tavily provider definition.
pub mod tavily;
/// Together AI provider definition.
pub mod together;
/// Triton provider definition.
pub mod triton;
/// Vertex AI provider definition.
pub mod vertex;
/// vLLM provider definition.
pub mod vllm;
/// Volcengine provider definition.
pub mod volcengine;
/// Voyage provider definition.
pub mod voyage;
/// WandB provider definition.
pub mod wandb;
/// WatsonX provider definition.
pub mod watsonx;
/// xAI provider definition.
pub mod xai;
/// Xiaomi Mimo provider definition.
pub mod xiaomi_mimo;
/// Xinference provider definition.
pub mod xinference;
/// Zhipu AI provider definition.
pub mod zhipuai;
