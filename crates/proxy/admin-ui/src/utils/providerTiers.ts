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

export interface TierGroup {
  tier: number
  label: string
  providers: CatalogProvider[]
}

export function groupByTier(providers: CatalogProvider[]): TierGroup[] {
  const buckets = new Map<number, CatalogProvider[]>()
  for (const p of providers) {
    const tier = PROVIDER_TIERS[p.id] ?? 3
    if (!buckets.has(tier)) buckets.set(tier, [])
    buckets.get(tier)!.push(p)
  }
  // Sort within each tier alphabetically by display_name
  for (const list of buckets.values()) {
    list.sort((a, b) => a.display_name.localeCompare(b.display_name))
  }
  return [...buckets.entries()]
    .sort(([a], [b]) => a - b)
    .map(([tier, provs]) => ({
      tier,
      label: TIER_LABELS[tier] ?? 'Other',
      providers: provs,
    }))
}
