# Provider Documentation

anyllm-proxy supports 74 providers. All OpenAI-compatible providers route through a single HTTP client — adding a new provider is metadata-only (no new HTTP code).

## Usage Patterns

**Single-backend** — set `BACKEND=<id>` and the provider's API key env var:
```bash
BACKEND=groq GROQ_API_KEY=your-key cargo run -p anyllm_proxy
```

**Multi-backend (LiteLLM YAML)** — set `PROXY_CONFIG=config.yaml`:
```yaml
model_list:
  - model_name: fast
    litellm_params:
      model: groq/llama-3.3-70b-versatile
      api_key: "env:GROQ_API_KEY"
  - model_name: smart
    litellm_params:
      model: anthropic/claude-3-5-sonnet-20241022
      api_key: "env:ANTHROPIC_API_KEY"
```

## Provider Index

### Implemented (fully live-tested)

| Provider | ID | Docs |
|---|---|---|
| OpenAI | `openai` | [openai.md](openai.md) |
| Anthropic | `anthropic` | [anthropic.md](anthropic.md) |
| Google AI Studio | `gemini` | [gemini.md](gemini.md) |

### Wired (HTTP client built, not live-tested)

| Provider | ID | Docs |
|---|---|---|
| Google Vertex AI | `vertex_ai` | [vertex_ai.md](vertex_ai.md) |
| Azure OpenAI | `azure` | [azure.md](azure.md) |
| AWS Bedrock | `bedrock` | [bedrock.md](bedrock.md) |

### Stub — Cloud (OpenAI-compatible)

| Provider | ID | Docs |
|---|---|---|
| xAI | `xai` | [xai.md](xai.md) |
| Groq | `groq` | [groq.md](groq.md) |
| Together AI | `together_ai` | [together_ai.md](together_ai.md) |
| OpenRouter | `openrouter` | [openrouter.md](openrouter.md) |
| Fireworks AI | `fireworks_ai` | [fireworks_ai.md](fireworks_ai.md) |
| Mistral AI | `mistral` | [mistral.md](mistral.md) |
| Codestral | `codestral` | [codestral.md](codestral.md) |
| Perplexity AI | `perplexity` | [perplexity.md](perplexity.md) |
| DeepSeek | `deepseek` | [deepseek.md](deepseek.md) |
| Cohere | `cohere_chat` | [cohere_chat.md](cohere_chat.md) |
| Cerebras | `cerebras` | [cerebras.md](cerebras.md) |
| SambaNova | `sambanova` | [sambanova.md](sambanova.md) |
| Nebius AI Studio | `nebius` | [nebius.md](nebius.md) |
| DeepInfra | `deepinfra` | [deepinfra.md](deepinfra.md) |
| Novita AI | `novita` | [novita.md](novita.md) |
| Databricks | `databricks` | [databricks.md](databricks.md) |
| Anyscale | `anyscale` | [anyscale.md](anyscale.md) |
| HuggingFace | `huggingface` | [huggingface.md](huggingface.md) |
| AI21 Labs | `ai21` | [ai21.md](ai21.md) |
| NVIDIA NIM | `nvidia_nim` | [nvidia_nim.md](nvidia_nim.md) |
| Moonshot AI | `moonshot` | [moonshot.md](moonshot.md) |
| Volcano Engine | `volcengine` | [volcengine.md](volcengine.md) |
| MiniMax | `minimax` | [minimax.md](minimax.md) |
| Z.ai / Zhipu AI | `zai` | [zhipuai.md](zhipuai.md) |
| Featherless AI | `featherless_ai` | [featherless_ai.md](featherless_ai.md) |
| FriendliAI | `friendliai` | [friendliai.md](friendliai.md) |
| Lambda AI | `lambda_ai` | [lambda_ai.md](lambda_ai.md) |
| Hyperbolic | `hyperbolic` | [hyperbolic.md](hyperbolic.md) |
| Nscale | `nscale` | [nscale.md](nscale.md) |
| GitHub Copilot / Models | `github_copilot` | [github.md](github.md) |
| Aleph Alpha | `aleph_alpha` | [aleph_alpha.md](aleph_alpha.md) |
| NLP Cloud | `nlp_cloud` | [nlp_cloud.md](nlp_cloud.md) |
| Clarifai | `clarifai` | [clarifai.md](clarifai.md) |
| Predibase | `predibase` | [predibase.md](predibase.md) |
| Replicate | `replicate` | [replicate.md](replicate.md) |
| Chutes AI | `chutes` | [chutes.md](chutes.md) |
| GMI Cloud | `gmi` | [gmi_cloud.md](gmi_cloud.md) |
| Meta Llama API | `meta_llama` | [meta_llama.md](meta_llama.md) |
| AI/ML API | `aiml` | [ai_ml_api.md](ai_ml_api.md) |
| Voyage AI | `voyage` | [voyage.md](voyage.md) |
| Scaleway | `scaleway` | [scaleway.md](scaleway.md) |
| Baseten | `baseten` | [baseten.md](baseten.md) |
| Dashscope (Qwen) | `dashscope` | [dashscope.md](dashscope.md) |
| Jina AI | `jina_ai` | [jina.md](jina.md) |
| OVHCloud | `ovhcloud` | [ovhcloud.md](ovhcloud.md) |
| Gradient AI | `gradient_ai` | [gradient_ai.md](gradient_ai.md) |
| Galadriel | `galadriel` | [galadriel.md](galadriel.md) |
| Morph | `morph` | [morph.md](morph.md) |
| Xiaomi MiMo | `xiaomi_mimo` | [xiaomi_mimo.md](xiaomi_mimo.md) |
| PublicAI | `publicai` | [public_ai.md](public_ai.md) |
| NanoGPT | `nanogpt` | [nanogpt.md](nanogpt.md) |
| W&B Inference | `wandb` | [wandb.md](wandb.md) |
| Bytez | `bytez` | [bytez.md](bytez.md) |

### Stub — Per-Instance URL (requires `api_base` or `OPENAI_BASE_URL`)

| Provider | ID | Docs |
|---|---|---|
| Azure AI Foundry | `azure_ai` | [azure_ai.md](azure_ai.md) |
| IBM WatsonX | `watsonx` | [watsonx.md](watsonx.md) |
| Cloudflare Workers AI | `cloudflare` | [cloudflare.md](cloudflare.md) |
| Snowflake Cortex | `snowflake` | [snowflake.md](snowflake.md) |

### Stub — Not Yet Routable

| Provider | ID | Docs | Reason |
|---|---|---|---|
| AWS SageMaker | `sagemaker` | [sagemaker.md](sagemaker.md) | Custom SigV4 protocol, no HTTP client |

### Stub — Local / Self-Hosted

| Provider | ID | Docs |
|---|---|---|
| Ollama | `ollama` | [ollama.md](ollama.md) |
| vLLM | `hosted_vllm` | [hosted_vllm.md](hosted_vllm.md) |
| LM Studio | `lm_studio` | [lm_studio.md](lm_studio.md) |
| llamafile | `llamafile` | [llamafile.md](llamafile.md) |
| Xinference | `xinference` | [xinference.md](xinference.md) |
| Petals | `petals` | [petals.md](petals.md) |
| NVIDIA Triton | `triton` | [triton.md](triton.md) |
| Infinity | `infinity` | [infinity.md](infinity.md) |
| Lemonade | `lemonade` | [lemonade.md](lemonade.md) |
| Docker Model Runner | `docker_model_runner` | [docker_model_runner.md](docker_model_runner.md) |
