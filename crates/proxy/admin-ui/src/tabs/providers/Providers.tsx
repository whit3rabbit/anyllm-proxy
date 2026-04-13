import { useState } from 'react'
import { useCatalogProviders, useRefreshProvider, useCatalogProviderModels } from '../../api/queries'
import type { CatalogProvider } from '../../api/types'

type CategoryFilter = 'all' | 'llm' | 'audio' | 'search' | 'image'

function getCategory(p: CatalogProvider): CategoryFilter {
  if (p.capabilities.chat_completions) return 'llm'
  const id = p.id.toLowerCase()
  if (['deepgram', 'elevenlabs', 'cartesia', 'playht', 'assemblyai'].includes(id)) return 'audio'
  if (['tavily', 'serper', 'exa', 'brave'].includes(id)) return 'search'
  if (['stability', 'stability_ai'].includes(id)) return 'image'
  return 'llm'
}

function formatLastRefreshed(ts: number | null): string {
  if (!ts) return 'never'
  const diff = Math.floor(Date.now() / 1000) - ts
  if (diff < 60) return 'just now'
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`
  return `${Math.floor(diff / 86400)}d ago`
}

function StatusBadge({ status }: { status: CatalogProvider['status'] }) {
  const colors: Record<string, string> = {
    implemented: '#22c55e',
    wired: '#f59e0b',
    stub: '#6b7280',
  }
  return (
    <span style={{
      display: 'inline-block',
      padding: '1px 6px',
      fontSize: 11,
      fontWeight: 600,
      borderRadius: 3,
      background: colors[status] + '22',
      color: colors[status],
      border: `1px solid ${colors[status]}44`,
      textTransform: 'uppercase',
      letterSpacing: '0.04em',
    }}>
      {status}
    </span>
  )
}

function ProviderRow({ provider }: { provider: CatalogProvider }) {
  const [expanded, setExpanded] = useState(false)
  const refresh = useRefreshProvider()
  const { data: models, isLoading: modelsLoading } = useCatalogProviderModels(expanded ? provider.id : null)

  const canRefresh = provider.capabilities.chat_completions
  const modelDisplay = provider.cached_model_count > 0 && provider.cached_model_count !== provider.model_count
    ? `${provider.model_count} (+${provider.cached_model_count} live)`
    : String(provider.model_count || '—')

  function handleRefresh(e: React.MouseEvent) {
    e.stopPropagation()
    refresh.mutate(provider.id)
  }

  return (
    <>
      <tr
        style={{ cursor: 'pointer', userSelect: 'none' }}
        onClick={() => setExpanded(v => !v)}
      >
        <td style={{ fontWeight: 500 }}>
          {expanded ? '▾ ' : '▸ '}{provider.display_name}
        </td>
        <td><StatusBadge status={provider.status} /></td>
        <td style={{ color: '#9ca3af', fontSize: 12 }}>{provider.protocol}</td>
        <td>{modelDisplay}</td>
        <td style={{ color: '#9ca3af', fontSize: 12 }}>{formatLastRefreshed(provider.last_refreshed)}</td>
        <td>
          {canRefresh && (
            <button
              className="btn btn-secondary btn-sm"
              onClick={handleRefresh}
              disabled={refresh.isPending}
              title="Fetch live model list from provider API"
            >
              {refresh.isPending ? '…' : 'Refresh'}
            </button>
          )}
        </td>
      </tr>
      {expanded && (
        <tr>
          <td colSpan={6} style={{ padding: '8px 16px 12px 28px', background: '#0d1117' }}>
            {modelsLoading && <span style={{ color: '#6b7280' }}>loading…</span>}
            {models && (
              <div>
                {models.models.length > 0 && (
                  <div style={{ marginBottom: 6 }}>
                    <span style={{ fontSize: 11, color: '#6b7280', textTransform: 'uppercase', letterSpacing: '0.06em' }}>Static models ({models.models.length})</span>
                    <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4, marginTop: 4 }}>
                      {(models.models as Array<{ id: string }>).map(m => (
                        <span key={m.id} style={{ fontFamily: 'monospace', fontSize: 11, padding: '1px 6px', background: '#1e2a3a', borderRadius: 2, color: '#93c5fd' }}>{m.id}</span>
                      ))}
                    </div>
                  </div>
                )}
                {models.cached_models && models.cached_models.length > 0 && (
                  <div>
                    <span style={{ fontSize: 11, color: '#6b7280', textTransform: 'uppercase', letterSpacing: '0.06em' }}>Live models ({models.cached_models.length})</span>
                    <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4, marginTop: 4 }}>
                      {models.cached_models.map(id => (
                        <span key={id} style={{ fontFamily: 'monospace', fontSize: 11, padding: '1px 6px', background: '#1a2e1a', borderRadius: 2, color: '#86efac' }}>{id}</span>
                      ))}
                    </div>
                  </div>
                )}
                {models.models.length === 0 && (!models.cached_models || models.cached_models.length === 0) && (
                  <span style={{ color: '#6b7280', fontSize: 12 }}>No models in catalog. {provider.capabilities.chat_completions ? 'Click Refresh to fetch from provider API.' : 'Not an LLM provider.'}</span>
                )}
              </div>
            )}
          </td>
        </tr>
      )}
    </>
  )
}

export default function Providers() {
  const { data: providers, isLoading, error } = useCatalogProviders()
  const [filter, setFilter] = useState<CategoryFilter>('all')
  const refresh = useRefreshProvider()

  const filtered = (providers ?? []).filter(p =>
    filter === 'all' || getCategory(p) === filter
  )

  const llmProviders = (providers ?? []).filter(p => p.capabilities.chat_completions)

  function handleRefreshAll() {
    for (const p of llmProviders) {
      refresh.mutate(p.id)
    }
  }

  const FILTERS: { id: CategoryFilter; label: string }[] = [
    { id: 'all', label: 'All' },
    { id: 'llm', label: 'LLM' },
    { id: 'audio', label: 'Audio' },
    { id: 'search', label: 'Search' },
    { id: 'image', label: 'Image' },
  ]

  return (
    <div>
      <div style={{ display: 'flex', alignItems: 'center', marginBottom: 16, gap: 12 }}>
        <span style={{ fontSize: 15, fontWeight: 600 }}>Provider Catalog</span>
        <div style={{ flex: 1 }} />
        <button
          className="btn btn-secondary btn-sm"
          onClick={handleRefreshAll}
          disabled={refresh.isPending}
          title="Refresh all LLM providers with configured API keys"
        >
          Refresh All LLM
        </button>
      </div>

      <div style={{ display: 'flex', gap: 6, marginBottom: 12 }}>
        {FILTERS.map(f => (
          <button
            key={f.id}
            className={`btn btn-sm ${filter === f.id ? 'btn-primary' : 'btn-secondary'}`}
            onClick={() => setFilter(f.id)}
          >
            {f.label}
          </button>
        ))}
      </div>

      {isLoading && <div style={{ color: '#6b7280' }}>Loading…</div>}
      {error && <div style={{ color: '#f87171' }}>Failed to load providers</div>}

      {!isLoading && filtered.length > 0 && (
        <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 13 }}>
          <thead>
            <tr style={{ color: '#6b7280', borderBottom: '1px solid #1e2a3a' }}>
              <th style={{ textAlign: 'left', padding: '6px 8px', fontWeight: 500 }}>Provider</th>
              <th style={{ textAlign: 'left', padding: '6px 8px', fontWeight: 500 }}>Status</th>
              <th style={{ textAlign: 'left', padding: '6px 8px', fontWeight: 500 }}>Protocol</th>
              <th style={{ textAlign: 'left', padding: '6px 8px', fontWeight: 500 }}>Models</th>
              <th style={{ textAlign: 'left', padding: '6px 8px', fontWeight: 500 }}>Last Refreshed</th>
              <th style={{ textAlign: 'left', padding: '6px 8px', fontWeight: 500 }} />
            </tr>
          </thead>
          <tbody>
            {filtered.map(p => (
              <ProviderRow key={p.id} provider={p} />
            ))}
          </tbody>
        </table>
      )}

      {!isLoading && filtered.length === 0 && (
        <div style={{ color: '#6b7280', padding: 20 }}>No providers in this category.</div>
      )}
    </div>
  )
}
