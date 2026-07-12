import type { CatalogProvider } from '../api/types'

// Hardcoded popularity tiers. Derived from market share, not model_count
// (stubs inflate counts). Providers not listed default to tier 3.
const PROVIDER_TIERS: Record<string, number> = {
  // Tier 0: dominant market share
  openai: 0, anthropic: 0, gemini: 0, vertex: 0,
  // Tier 1: major cloud / high-growth
  azure: 1, bedrock: 1, mistral: 1, groq: 1, deepseek: 1, xai: 1,
  // Tier 2: well-known alternatives
  together_ai: 2, openrouter: 2, fireworks_ai: 2, perplexity: 2,
  cohere_chat: 2, cerebras: 2, sambanova: 2, ollama: 2,
  deepinfra: 2, replicate: 2, nvidia_nim: 2,
}

const TIER_LABELS: Record<number, string> = {
  0: 'Top providers',
  1: 'Popular',
  2: 'Notable',
  3: 'More providers',
}

// Providers with a free tier (API key still required). Curated from provider docs.
const FREE_PROVIDERS = new Set([
  'gemini', 'groq', 'openrouter', 'mistral', 'deepseek', 'cohere_chat', 'cohere',
])

// A provider is "local" when its default endpoint points at the loopback address —
// ollama, lm_studio, llamafile, vllm, etc. Same rule the backend uses to advertise them.
function isLocal(p: CatalogProvider): boolean {
  const base = p.default_base_url ?? ''
  return /localhost|127\.0\.0\.1|0\.0\.0\.0/.test(base)
}

export interface SectionGroup {
  key: string
  label: string
  top: boolean // render with the larger "top" grid
  providers: CatalogProvider[]
}

// Group providers into ordered sections. Each provider lands in exactly one section,
// first match wins: Favorite > Local > Free > popularity tier.
export function groupSections(
  providers: CatalogProvider[],
  favoriteIds: Set<string>,
): SectionGroup[] {
  const favorites: CatalogProvider[] = []
  const local: CatalogProvider[] = []
  const free: CatalogProvider[] = []
  const tiers = new Map<number, CatalogProvider[]>()

  for (const p of providers) {
    if (favoriteIds.has(p.id)) favorites.push(p)
    else if (isLocal(p)) local.push(p)
    else if (FREE_PROVIDERS.has(p.id)) free.push(p)
    else {
      const tier = PROVIDER_TIERS[p.id] ?? 3
      if (!tiers.has(tier)) tiers.set(tier, [])
      tiers.get(tier)!.push(p)
    }
  }

  const byName = (a: CatalogProvider, b: CatalogProvider) =>
    a.display_name.localeCompare(b.display_name)
  favorites.sort(byName)
  local.sort(byName)
  free.sort(byName)

  const out: SectionGroup[] = []
  if (favorites.length) out.push({ key: 'favorites', label: 'Favorites', top: true, providers: favorites })
  if (local.length) out.push({ key: 'local', label: 'Local LLMs', top: false, providers: local })
  if (free.length) out.push({ key: 'free', label: 'Free', top: false, providers: free })
  for (const [tier, provs] of [...tiers.entries()].sort(([a], [b]) => a - b)) {
    provs.sort(byName)
    out.push({ key: `tier-${tier}`, label: TIER_LABELS[tier] ?? 'Other', top: false, providers: provs })
  }
  return out
}
