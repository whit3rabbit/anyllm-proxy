// Pull model-name strings out of a catalog provider-models response (static
// catalog models + live cached model ids), deduped. Shared by the Router tier
// form and the Routes provider picker.
export function catalogModelIds(data: unknown): string[] {
  if (!data || typeof data !== 'object') return []
  const d = data as { models?: unknown[]; cached_models?: string[] }
  const out = new Set<string>()
  for (const m of d.models ?? []) {
    if (m && typeof m === 'object') {
      const id = (m as { id?: string; name?: string }).id ?? (m as { name?: string }).name
      if (id) out.add(id)
    }
  }
  for (const c of d.cached_models ?? []) out.add(c)
  return [...out]
}
