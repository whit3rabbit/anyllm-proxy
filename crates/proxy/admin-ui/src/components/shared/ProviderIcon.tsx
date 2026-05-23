import { type ComponentType, type SVGProps, memo } from 'react'

// Import the Mono (SVG-only) sub-component directly to avoid pulling in
// @lobehub/ui (peer dep) through the compound index → Avatar path.
import Ai21 from '@lobehub/icons/es/Ai21/components/Mono'
import AlephAlpha from '@lobehub/icons/es/AlephAlpha/components/Mono'
import Anthropic from '@lobehub/icons/es/Anthropic/components/Mono'
import Anyscale from '@lobehub/icons/es/Anyscale/components/Mono'
import AssemblyAI from '@lobehub/icons/es/AssemblyAI/components/Mono'
import Aws from '@lobehub/icons/es/Aws/components/Mono'
import Azure from '@lobehub/icons/es/Azure/components/Mono'
import AzureAI from '@lobehub/icons/es/AzureAI/components/Mono'
import Baidu from '@lobehub/icons/es/Baidu/components/Mono'
import Baseten from '@lobehub/icons/es/Baseten/components/Mono'
import Bedrock from '@lobehub/icons/es/Bedrock/components/Mono'
import Cerebras from '@lobehub/icons/es/Cerebras/components/Mono'
import Cloudflare from '@lobehub/icons/es/Cloudflare/components/Mono'
import Cohere from '@lobehub/icons/es/Cohere/components/Mono'
import DeepInfra from '@lobehub/icons/es/DeepInfra/components/Mono'
import DeepSeek from '@lobehub/icons/es/DeepSeek/components/Mono'
import ElevenLabs from '@lobehub/icons/es/ElevenLabs/components/Mono'
import Exa from '@lobehub/icons/es/Exa/components/Mono'
import Featherless from '@lobehub/icons/es/Featherless/components/Mono'
import Fireworks from '@lobehub/icons/es/Fireworks/components/Mono'
import Friendli from '@lobehub/icons/es/Friendli/components/Mono'
import Gemini from '@lobehub/icons/es/Gemini/components/Mono'
import Github from '@lobehub/icons/es/Github/components/Mono'
import Groq from '@lobehub/icons/es/Groq/components/Mono'
import HuggingFace from '@lobehub/icons/es/HuggingFace/components/Mono'
import Hyperbolic from '@lobehub/icons/es/Hyperbolic/components/Mono'
import IFlyTekCloud from '@lobehub/icons/es/IFlyTekCloud/components/Mono'
import Jina from '@lobehub/icons/es/Jina/components/Mono'
import Lambda from '@lobehub/icons/es/Lambda/components/Mono'
import LmStudio from '@lobehub/icons/es/LmStudio/components/Mono'
import Meta from '@lobehub/icons/es/Meta/components/Mono'
import Minimax from '@lobehub/icons/es/Minimax/components/Mono'
import Mistral from '@lobehub/icons/es/Mistral/components/Mono'
import Moonshot from '@lobehub/icons/es/Moonshot/components/Mono'
import Morph from '@lobehub/icons/es/Morph/components/Mono'
import Nebius from '@lobehub/icons/es/Nebius/components/Mono'
import Novita from '@lobehub/icons/es/Novita/components/Mono'
import Nvidia from '@lobehub/icons/es/Nvidia/components/Mono'
import Ollama from '@lobehub/icons/es/Ollama/components/Mono'
import OpenAI from '@lobehub/icons/es/OpenAI/components/Mono'
import OpenRouter from '@lobehub/icons/es/OpenRouter/components/Mono'
import Perplexity from '@lobehub/icons/es/Perplexity/components/Mono'
import Pollinations from '@lobehub/icons/es/Pollinations/components/Mono'
import Qwen from '@lobehub/icons/es/Qwen/components/Mono'
import Replicate from '@lobehub/icons/es/Replicate/components/Mono'
import SambaNova from '@lobehub/icons/es/SambaNova/components/Mono'
import SiliconCloud from '@lobehub/icons/es/SiliconCloud/components/Mono'
import Snowflake from '@lobehub/icons/es/Snowflake/components/Mono'
import Spark from '@lobehub/icons/es/Spark/components/Mono'
import Stability from '@lobehub/icons/es/Stability/components/Mono'
import Tavily from '@lobehub/icons/es/Tavily/components/Mono'
import Together from '@lobehub/icons/es/Together/components/Mono'
import VertexAI from '@lobehub/icons/es/VertexAI/components/Mono'
import Vllm from '@lobehub/icons/es/Vllm/components/Mono'
import Volcengine from '@lobehub/icons/es/Volcengine/components/Mono'
import Voyage from '@lobehub/icons/es/Voyage/components/Mono'
import XAI from '@lobehub/icons/es/XAI/components/Mono'
import XiaomiMiMo from '@lobehub/icons/es/XiaomiMiMo/components/Mono'
import Xinference from '@lobehub/icons/es/Xinference/components/Mono'
import ZAI from '@lobehub/icons/es/ZAI/components/Mono'
import Zhipu from '@lobehub/icons/es/Zhipu/components/Mono'

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type LobehubIcon = ComponentType<SVGProps<SVGSVGElement> & { size?: number | string }>

// Maps ProviderDef.id → lobehub Mono icon component.
// IDs not listed fall back to the letter avatar.
const ICON_MAP: Record<string, LobehubIcon> = {
  // Core / implemented
  openai: OpenAI,
  anthropic: Anthropic,
  gemini: Gemini,
  vertex: VertexAI,
  vertex_ai: VertexAI,
  azure: Azure,
  azure_ai: AzureAI,
  bedrock: Bedrock,
  sagemaker: Aws,
  // LLM stubs
  groq: Groq,
  together_ai: Together,
  openrouter: OpenRouter,
  fireworks_ai: Fireworks,
  mistral: Mistral,
  codestral: Mistral,
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
  friendliai: Friendli,
  lambda: Lambda,
  hyperbolic: Hyperbolic,
  github_copilot: Github,
  github: Github,
  aleph_alpha: AlephAlpha,
  replicate: Replicate,
  meta_llama: Meta,
  voyage: Voyage,
  baseten: Baseten,
  lm_studio: LmStudio,
  xinference: Xinference,
  cloudflare: Cloudflare,
  snowflake: Snowflake,
  dashscope: Qwen,
  jina_ai: Jina,
  jina: Jina,
  morph: Morph,
  xiaomi_mimo: XiaomiMiMo,
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

function LetterAvatar({ id, size }: { id: string; size: number }) {
  const hue = idToHue(id)
  const letter = id.replace(/[^a-zA-Z]/, '')[0]?.toUpperCase() ?? '?'
  return (
    <span
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
  const IconComponent = ICON_MAP[id]
  if (IconComponent) {
    return (
      <span
        className={className}
        style={{ display: 'inline-flex', alignItems: 'center', flexShrink: 0, ...style }}
      >
        <IconComponent size={size} />
      </span>
    )
  }
  return <LetterAvatar id={id} size={size} />
})

export default ProviderIcon
