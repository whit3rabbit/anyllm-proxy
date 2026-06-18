import { type ComponentType, memo } from 'react'

import Ai21 from '@lobehub/icons/es/Ai21/components/Avatar'
import AlephAlpha from '@lobehub/icons/es/AlephAlpha/components/Avatar'
import Anthropic from '@lobehub/icons/es/Anthropic/components/Avatar'
import Anyscale from '@lobehub/icons/es/Anyscale/components/Avatar'
import AssemblyAI from '@lobehub/icons/es/AssemblyAI/components/Avatar'
import Aws from '@lobehub/icons/es/Aws/components/Avatar'
import Azure from '@lobehub/icons/es/Azure/components/Avatar'
import AzureAI from '@lobehub/icons/es/AzureAI/components/Avatar'
import Baidu from '@lobehub/icons/es/Baidu/components/Avatar'
import Baseten from '@lobehub/icons/es/Baseten/components/Avatar'
import Bedrock from '@lobehub/icons/es/Bedrock/components/Avatar'
import Bfl from '@lobehub/icons/es/Bfl/components/Avatar'
import Cerebras from '@lobehub/icons/es/Cerebras/components/Avatar'
import Cloudflare from '@lobehub/icons/es/Cloudflare/components/Avatar'
import Cohere from '@lobehub/icons/es/Cohere/components/Avatar'
import Crusoe from '@lobehub/icons/es/Crusoe/components/Avatar'
import Dbrx from '@lobehub/icons/es/Dbrx/components/Avatar'
import DeepInfra from '@lobehub/icons/es/DeepInfra/components/Avatar'
import DeepSeek from '@lobehub/icons/es/DeepSeek/components/Avatar'
import ElevenLabs from '@lobehub/icons/es/ElevenLabs/components/Avatar'
import Exa from '@lobehub/icons/es/Exa/components/Avatar'
import Fal from '@lobehub/icons/es/Fal/components/Avatar'
import Featherless from '@lobehub/icons/es/Featherless/components/Avatar'
import Fireworks from '@lobehub/icons/es/Fireworks/components/Avatar'
import Friendli from '@lobehub/icons/es/Friendli/components/Avatar'
import Gemini from '@lobehub/icons/es/Gemini/components/Avatar'
import Github from '@lobehub/icons/es/Github/components/Avatar'
import Google from '@lobehub/icons/es/Google/components/Avatar'
import Groq from '@lobehub/icons/es/Groq/components/Avatar'
import HuggingFace from '@lobehub/icons/es/HuggingFace/components/Avatar'
import Hyperbolic from '@lobehub/icons/es/Hyperbolic/components/Avatar'
import IBM from '@lobehub/icons/es/IBM/components/Avatar'
import IFlyTekCloud from '@lobehub/icons/es/IFlyTekCloud/components/Avatar'
import Jina from '@lobehub/icons/es/Jina/components/Avatar'
import Lambda from '@lobehub/icons/es/Lambda/components/Avatar'
import LmStudio from '@lobehub/icons/es/LmStudio/components/Avatar'
import Meta from '@lobehub/icons/es/Meta/components/Avatar'
import Minimax from '@lobehub/icons/es/Minimax/components/Avatar'
import Mistral from '@lobehub/icons/es/Mistral/components/Avatar'
import Moonshot from '@lobehub/icons/es/Moonshot/components/Avatar'
import Morph from '@lobehub/icons/es/Morph/components/Avatar'
import Nebius from '@lobehub/icons/es/Nebius/components/Avatar'
import Nova from '@lobehub/icons/es/Nova/components/Avatar'
import Novita from '@lobehub/icons/es/Novita/components/Avatar'
import NPLCloud from '@lobehub/icons/es/NPLCloud/components/Avatar'
import Nvidia from '@lobehub/icons/es/Nvidia/components/Avatar'
import Ollama from '@lobehub/icons/es/Ollama/components/Avatar'
import OpenAI from '@lobehub/icons/es/OpenAI/components/Avatar'
import OpenRouter from '@lobehub/icons/es/OpenRouter/components/Avatar'
import PaLM from '@lobehub/icons/es/PaLM/components/Avatar'
import Perplexity from '@lobehub/icons/es/Perplexity/components/Avatar'
import Pollinations from '@lobehub/icons/es/Pollinations/components/Avatar'
import Qwen from '@lobehub/icons/es/Qwen/components/Avatar'
import Recraft from '@lobehub/icons/es/Recraft/components/Avatar'
import Replicate from '@lobehub/icons/es/Replicate/components/Avatar'
import Runway from '@lobehub/icons/es/Runway/components/Avatar'
import SambaNova from '@lobehub/icons/es/SambaNova/components/Avatar'
import SiliconCloud from '@lobehub/icons/es/SiliconCloud/components/Avatar'
import Snowflake from '@lobehub/icons/es/Snowflake/components/Avatar'
import Spark from '@lobehub/icons/es/Spark/components/Avatar'
import Stability from '@lobehub/icons/es/Stability/components/Avatar'
import Tavily from '@lobehub/icons/es/Tavily/components/Avatar'
import Together from '@lobehub/icons/es/Together/components/Avatar'
import V0 from '@lobehub/icons/es/V0/components/Avatar'
import Vercel from '@lobehub/icons/es/Vercel/components/Avatar'
import VertexAI from '@lobehub/icons/es/VertexAI/components/Avatar'
import Vllm from '@lobehub/icons/es/Vllm/components/Avatar'
import Volcengine from '@lobehub/icons/es/Volcengine/components/Avatar'
import Voyage from '@lobehub/icons/es/Voyage/components/Avatar'
import XAI from '@lobehub/icons/es/XAI/components/Avatar'
import XiaomiMiMo from '@lobehub/icons/es/XiaomiMiMo/components/Avatar'
import Xinference from '@lobehub/icons/es/Xinference/components/Avatar'
import ZAI from '@lobehub/icons/es/ZAI/components/Avatar'
import Zhipu from '@lobehub/icons/es/Zhipu/components/Avatar'

type LobehubAvatar = ComponentType<{
  size: number
  shape?: 'circle' | 'square'
  style?: React.CSSProperties
  className?: string
}>

// Maps ProviderDef.id to lobehub Avatar component.
// IDs not listed fall back to the letter avatar.
const ICON_MAP: Record<string, LobehubAvatar> = {
  // Core / implemented
  openai: OpenAI,
  anthropic: Anthropic,
  gemini: Gemini,
  vertex: VertexAI,
  vertex_ai: VertexAI,
  azure: Azure,
  azure_ai: AzureAI,
  azure_text: Azure,
  bedrock: Bedrock,
  bedrock_converse: Bedrock,
  bedrock_mantle: Bedrock,
  aws_polly: Aws,
  amazon_nova: Nova,
  sagemaker: Aws,
  // LLM stubs
  groq: Groq,
  together_ai: Together,
  openrouter: OpenRouter,
  fireworks_ai: Fireworks,
  'fireworks_ai-embedding-models': Fireworks,
  mistral: Mistral,
  codestral: Mistral,
  'text-completion-codestral': Mistral,
  perplexity: Perplexity,
  deepseek: DeepSeek,
  cerebras: Cerebras,
  ollama: Ollama,
  vllm: Vllm,
  sambanova: SambaNova,
  nebius: Nebius,
  deepinfra: DeepInfra,
  novita: Novita,
  cohere_chat: Cohere,
  ai21: Ai21,
  huggingface: HuggingFace,
  anyscale: Anyscale,
  xai: XAI,
  nvidia_nim: Nvidia,
  moonshot: Moonshot,
  volcengine: Volcengine,
  minimax: Minimax,
  zai: ZAI,
  zhipuai: ZAI,
  featherless: Featherless,
  featherless_ai: Featherless,
  friendliai: Friendli,
  lambda: Lambda,
  lambda_ai: Lambda,
  hyperbolic: Hyperbolic,
  github_copilot: Github,
  github: Github,
  aleph_alpha: AlephAlpha,
  replicate: Replicate,
  meta_llama: Meta,
  voyage: Voyage,
  baseten: Baseten,
  black_forest_labs: Bfl,
  chatgpt: OpenAI,
  cohere: Cohere,
  crusoe: Crusoe,
  databricks: Dbrx,
  fal_ai: Fal,
  google_pse: Google,
  lm_studio: LmStudio,
  xinference: Xinference,
  cloudflare: Cloudflare,
  snowflake: Snowflake,
  dashscope: Qwen,
  jina_ai: Jina,
  jina: Jina,
  morph: Morph,
  xiaomi_mimo: XiaomiMiMo,
  nlp_cloud: NPLCloud,
  palm: PaLM,
  recraft: Recraft,
  runwayml: Runway,
  'text-completion-openai': OpenAI,
  v0: V0,
  vercel_ai_gateway: Vercel,
  watsonx: IBM,
  'vertex_ai-ai21_models': VertexAI,
  'vertex_ai-anthropic_models': VertexAI,
  'vertex_ai-deepseek_models': VertexAI,
  'vertex_ai-embedding-models': VertexAI,
  'vertex_ai-image-models': VertexAI,
  'vertex_ai-language-models': VertexAI,
  'vertex_ai-llama_models': VertexAI,
  'vertex_ai-minimax_models': VertexAI,
  'vertex_ai-mistral_models': VertexAI,
  'vertex_ai-moonshot_models': VertexAI,
  'vertex_ai-openai_models': VertexAI,
  'vertex_ai-qwen_models': VertexAI,
  'vertex_ai-text-models': VertexAI,
  'vertex_ai-video-models': VertexAI,
  'vertex_ai-zai_models': VertexAI,
  // OmniRoute parity stubs
  siliconflow: SiliconCloud,
  pollinations: Pollinations,
  stability: Stability,
  stability_ai: Stability,
  iflytek: IFlyTekCloud,
  baidu: Baidu,
  assemblyai: AssemblyAI,
  elevenlabs: ElevenLabs,
  tavily: Tavily,
  exa_ai: Exa,
  exa: Exa,
  aiml: OpenAI,
  gmi: OpenAI,
  publicai: OpenAI,
  // Extra aliases
  spark: Spark,
  zhipu: Zhipu,
}

// Derives a stable hue from the provider ID string for letter avatars.
function idToHue(id: string): number {
  let h = 0
  for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) & 0xffff
  return h % 360
}

function LetterAvatar({
  id,
  size,
  style,
  className,
}: {
  id: string
  size: number
  style?: React.CSSProperties
  className?: string
}) {
  const hue = idToHue(id)
  const letter = id.replace(/[^a-zA-Z]/, '')[0]?.toUpperCase() ?? '?'
  return (
    <span
      className={className}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        width: size,
        height: size,
        borderRadius: 4,
        background: `hsl(${hue}, 45%, 28%)`,
        color: `hsl(${hue}, 70%, 80%)`,
        fontSize: Math.round(size * 0.6),
        fontWeight: 600,
        lineHeight: 1,
        flexShrink: 0,
        userSelect: 'none',
        ...style,
      }}
    >
      {letter}
    </span>
  )
}

interface ProviderIconProps {
  id: string
  size?: number
  style?: React.CSSProperties
  className?: string
}

const ProviderIcon = memo(function ProviderIcon({
  id,
  size = 20,
  style,
  className,
}: ProviderIconProps) {
  const AvatarComponent = ICON_MAP[id]
  if (AvatarComponent) {
    return (
      <span
        className={className}
        style={{
          display: 'inline-flex',
          alignItems: 'center',
          flexShrink: 0,
          ...style,
        }}
      >
        <AvatarComponent
          size={size}
          shape="square"
          style={{
            borderRadius: Math.max(4, Math.round(size * 0.16)),
          }}
        />
      </span>
    )
  }
  return <LetterAvatar id={id} size={size} className={className} style={style} />
})

export default ProviderIcon
