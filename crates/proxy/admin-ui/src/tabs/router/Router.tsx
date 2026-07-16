import { useEffect, useState } from 'react'
import {
  useConfig,
  useSaveConfig,
  useManagedBackends,
  useCatalogProviderModels,
} from '../../api/queries'
import type { ConfigResponse, RouterConfig, RouterTierTarget, ManagedBackend } from '../../api/types'
import AsyncBoundary from '../../components/shared/AsyncBoundary'
import { AdminButton, AdminSurface } from '../../components/shared/Performative'
import { catalogModelIds } from '../../utils/catalogModels'

// The six Claude Code request tiers, in the order they are shown (and the same
// precedence the backend applies: image > web_search > think > long_context >
// background > default).
const TIERS: { key: keyof RouterConfig; label: string; hint: string }[] = [
  { key: 'default', label: 'Default', hint: 'Fallback when no other tier matches' },
  { key: 'background', label: 'Background', hint: 'Small/cheap model (Claude Code uses haiku)' },
  { key: 'think', label: 'Think', hint: 'Extended thinking enabled' },
  { key: 'long_context', label: 'Long Context', hint: 'Request tokens over the threshold' },
  { key: 'web_search', label: 'Web Search', hint: 'Request includes a web-search tool' },
  { key: 'image', label: 'Image', hint: 'Request includes image content' },
]

export default function Router() {
  const configQuery = useConfig()
  const { data: backendsData } = useManagedBackends()
  const backends = backendsData?.backends ?? []

  return (
    <div>
      <div className="section-header">
        <h2>Auto Router</h2>
      </div>
      <p style={{ color: 'var(--text-2)', marginTop: -4, marginBottom: 14 }}>
        Route Claude Code request tiers to a specific backend and model. Applies to both{' '}
        <code>/v1/messages</code> and <code>/v1/chat/completions</code> (the Long Context tier is
        evaluated on <code>/v1/messages</code> only). Disabled by default.
      </p>
      <AsyncBoundary query={configQuery} errorTitle="Failed to load router config">
        {(config: ConfigResponse) => <RouterForm config={config.router} backends={backends} />}
      </AsyncBoundary>
    </div>
  )
}

function RouterForm({ config, backends }: { config: RouterConfig; backends: ManagedBackend[] }) {
  const save = useSaveConfig()
  const [draft, setDraft] = useState<RouterConfig>(config)

  // Re-seed when the server config changes (e.g. another admin session saves).
  useEffect(() => {
    setDraft(config)
  }, [config])

  function setTier(key: keyof RouterConfig, patch: Partial<RouterTierTarget>) {
    setDraft(prev => ({
      ...prev,
      [key]: { ...(prev[key] as RouterTierTarget), ...patch },
    }))
  }

  function handleSave() {
    save.mutate({ router: draft })
  }

  return (
    <AdminSurface>
      <div className="form-group" style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        <label style={{ display: 'flex', alignItems: 'center', gap: 8, fontWeight: 600 }}>
          <input
            type="checkbox"
            checked={draft.enabled}
            onChange={e => setDraft({ ...draft, enabled: e.target.checked })}
          />
          Enable router
        </label>
        <span style={{ marginLeft: 'auto', display: 'flex', alignItems: 'center', gap: 8 }}>
          <span className="form-label" style={{ margin: 0 }}>Context Threshold</span>
          <input
            type="number"
            min={0}
            // Backend stores this as a u32; clamp so a huge value can't fail the
            // whole PUT with a serde overflow error.
            max={4294967295}
            value={draft.context_threshold}
            onChange={e =>
              setDraft({
                ...draft,
                context_threshold: Math.min(Math.max(Number(e.target.value) || 0, 0), 4294967295),
              })
            }
            style={{ width: 120 }}
          />
        </span>
      </div>

      <div className="router-tiers">
        {TIERS.map(t => (
          <TierRow
            key={t.key}
            label={t.label}
            hint={t.hint}
            target={draft[t.key] as RouterTierTarget}
            backends={backends}
            onChange={patch => setTier(t.key, patch)}
          />
        ))}
      </div>

      <div style={{ marginTop: 16 }}>
        <AdminButton tone="primary" loading={save.isPending} onClick={handleSave}>
          Save router
        </AdminButton>
      </div>
    </AdminSurface>
  )
}

function TierRow({
  label,
  hint,
  target,
  backends,
  onChange,
}: {
  label: string
  hint: string
  target: RouterTierTarget
  backends: ManagedBackend[]
  onChange: (patch: Partial<RouterTierTarget>) => void
}) {
  const selected = backends.find(b => b.name === target.backend_name)
  // Model suggestions for the chosen backend's provider. Editable input (not a
  // strict <select>) with a datalist so local LLMs with no catalog still work.
  const modelsQuery = useCatalogProviderModels(selected?.provider_id ?? null)
  const models = catalogModelIds(modelsQuery.data)
  const listId = `models-${label.replace(/\s+/g, '-')}`

  return (
    <div className="form-group router-tier">
      <div className="router-tier-head">
        <label style={{ display: 'flex', alignItems: 'center', gap: 8, fontWeight: 600 }}>
          <input
            type="checkbox"
            checked={target.enabled}
            onChange={e => onChange({ enabled: e.target.checked })}
          />
          {label}
        </label>
        <span className="router-tier-hint">{hint}</span>
      </div>
      <div className="form-row" style={{ gap: 8 }}>
        <select
          value={target.backend_name}
          onChange={e => onChange({ backend_name: e.target.value })}
          style={{ flex: 1 }}
        >
          <option value="">Select backend…</option>
          {backends.map(b => (
            <option key={b.name} value={b.name}>
              {b.name} ({b.provider_id})
            </option>
          ))}
        </select>
        <input
          type="text"
          list={listId}
          placeholder="model"
          value={target.model}
          onChange={e => onChange({ model: e.target.value })}
          style={{ flex: 1 }}
        />
        <datalist id={listId}>
          {models.map(m => (
            <option key={m} value={m} />
          ))}
        </datalist>
      </div>
    </div>
  )
}
