import { useEffect, useMemo, useState } from 'react'
import {
  useCatalogProviders,
  useManagedBackends,
  useCreateManagedBackend,
  useDeleteManagedBackend,
  useBackends,
} from '../../api/queries'
import type { CatalogProvider, ManagedBackend } from '../../api/types'
import { getProviderFields } from '../../utils/providerFields'
import { groupByTier } from '../../utils/providerTiers'
import AsyncBoundary from '../../components/shared/AsyncBoundary'
import ConfirmDialog from '../../components/shared/ConfirmDialog'
import StatusDot from '../../components/shared/StatusDot'
import ProviderIcon from '../../components/shared/ProviderIcon'

// ── Provider Tile ──────────────────────────────────────────────────────────────

function ProviderTile({
  provider,
  backendCount,
  onClick,
}: {
  provider: CatalogProvider
  backendCount: number
  onClick: () => void
}) {
  return (
    <button
      type="button"
      className={`provider-tile${backendCount > 0 ? ' has-backends' : ''}`}
      onClick={onClick}
    >
      <ProviderIcon id={provider.id} size={28} />
      <span className="provider-tile-name">{provider.display_name}</span>
      {backendCount > 0 && (
        <span className="provider-tile-count">
          {backendCount} key{backendCount !== 1 ? 's' : ''}
        </span>
      )}
    </button>
  )
}

// ── Backend Row (inside detail panel) ──────────────────────────────────────────

function BackendRow({
  backend,
  healthStatus,
  onDelete,
}: {
  backend: ManagedBackend
  healthStatus?: string
  onDelete: () => void
}) {
  return (
    <div className="provider-backend-row">
      <StatusDot
        status={healthStatus === 'ok' ? 'ok' : healthStatus ? 'err' : 'dim'}
        pulse={healthStatus === 'ok'}
      />
      <span className="backend-name">{backend.name}</span>
      <span className="backend-status">
        {backend.api_key_set ? 'key set' : 'no key'}
        {backend.rpm != null && <> &middot; RPM {backend.rpm}</>}
      </span>
      <button className="btn btn-danger btn-sm" onClick={onDelete}>
        Delete
      </button>
    </div>
  )
}

// ── Add Backend Form (inside detail panel) ─────────────────────────────────────

function AddBackendForm({
  provider,
  existingCount,
  onCreated,
}: {
  provider: CatalogProvider
  existingCount: number
  onCreated: () => void
}) {
  const create = useCreateManagedBackend()
  const [open, setOpen] = useState(false)
  const [form, setForm] = useState<Record<string, string>>(() => ({
    name: `${provider.id}-${existingCount + 1}`,
    provider_id: provider.id,
  }))

  // Reset form when the provider or count changes
  useEffect(() => {
    setForm({ name: `${provider.id}-${existingCount + 1}`, provider_id: provider.id })
    setOpen(false)
  }, [provider.id, existingCount])

  function submit() {
    create.mutate(
      {
        name: form.name,
        provider_id: provider.id,
        api_key: form.api_key || undefined,
        api_base: form.api_base || undefined,
        deployment: form.deployment || undefined,
        api_version: form.api_version || undefined,
        project: form.project || undefined,
        region: form.region || undefined,
        aws_access_key_id: form.aws_access_key_id || undefined,
        aws_secret_access_key: form.aws_secret_access_key || undefined,
        aws_session_token: form.aws_session_token || undefined,
        rpm: form.rpm ? Number(form.rpm) : undefined,
        tpm: form.tpm ? Number(form.tpm) : undefined,
      },
      {
        onSuccess: () => {
          setOpen(false)
          onCreated()
        },
      },
    )
  }

  if (!open) {
    return (
      <div className="provider-add-toggle">
        <button className="btn btn-primary btn-sm" onClick={() => setOpen(true)}>
          + Add key
        </button>
      </div>
    )
  }

  const fields = getProviderFields(provider)

  return (
    <div className="provider-add-form">
      <div className="form-group">
        <label className="form-label" htmlFor="add-backend-name">Name</label>
        <input
          id="add-backend-name"
          name="name"
          type="text"
          value={form.name}
          onChange={(e) => setForm((p) => ({ ...p, name: e.target.value }))}
          style={{ width: '100%' }}
        />
      </div>
      {fields.map((f) => (
        <div key={f.name} className="form-group">
          <label className="form-label" htmlFor={`add-${f.name}`}>{f.label}</label>
          {f.hint && <div className="form-hint">{f.hint}</div>}
          <input
            id={`add-${f.name}`}
            name={f.name}
            type={f.type}
            placeholder={f.placeholder}
            value={form[f.name] ?? ''}
            onChange={(e) => setForm((p) => ({ ...p, [f.name]: e.target.value }))}
            style={{ width: '100%' }}
          />
        </div>
      ))}
      {create.isError && <div className="inline-error">Failed to create backend</div>}
      <div className="provider-add-actions">
        <button
          className="btn btn-secondary btn-sm"
          onClick={() => setOpen(false)}
          disabled={create.isPending}
        >
          Cancel
        </button>
        <button
          className="btn btn-primary btn-sm"
          onClick={submit}
          disabled={!form.name || create.isPending}
        >
          {create.isPending ? 'Creating...' : 'Create'}
        </button>
      </div>
    </div>
  )
}

// ── Provider Detail Panel (lightbox overlay) ───────────────────────────────────

function ProviderDetailPanel({
  provider,
  backends,
  healthMap,
  onClose,
  onDeleteBackend,
}: {
  provider: CatalogProvider
  backends: ManagedBackend[]
  healthMap: Map<string, string>
  onClose: () => void
  onDeleteBackend: (b: ManagedBackend) => void
}) {
  // ESC to close
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape') onClose()
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [onClose])

  // Lock body scroll
  useEffect(() => {
    const prev = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    return () => { document.body.style.overflow = prev }
  }, [])

  const caps = provider.capabilities
  const capList: [string, boolean][] = [
    ['chat', caps.chat_completions],
    ['streaming', caps.streaming],
    ['tool use', caps.tool_use],
    ['vision', caps.vision],
    ['embeddings', caps.embeddings],
    ['batch', caps.batch],
  ]

  return (
    <>
      <div className="provider-scrim" onClick={onClose} />
      <div className="provider-panel" role="dialog" aria-modal="true" aria-label={provider.display_name}>
        {/* Header */}
        <div className="provider-panel-header">
          <ProviderIcon id={provider.id} size={36} />
          <h3>{provider.display_name}</h3>
          <button type="button" className="provider-panel-close" onClick={onClose} aria-label="Close">
            &times;
          </button>
        </div>

        {/* Capabilities */}
        <div className="provider-panel-caps">
          {capList.map(([label, active]) => (
            <span key={label} className={`badge-cap${active ? ' active' : ''}`}>
              {label}
            </span>
          ))}
          <span style={{ marginLeft: 'auto' }} className="badge-cap active">
            {provider.model_count} models
          </span>
        </div>

        {/* Meta */}
        <div className="provider-panel-meta">
          <span>
            Protocol: <span className="mono">{provider.protocol.replace(/_/g, ' ')}</span>
          </span>
          <span>
            Auth: <span className="mono">{provider.auth.replace(/_/g, ' ')}</span>
          </span>
          <span>
            Status: <span className="mono">{provider.status}</span>
          </span>
          {provider.env_vars.length > 0 && (
            <span>
              Env: <span className="mono">{provider.env_vars[0]}</span>
            </span>
          )}
        </div>

        {/* Configured keys */}
        <div className="provider-panel-section">
          <div className="provider-panel-section-label">
            Configured keys ({backends.length})
          </div>
          {backends.length === 0 && (
            <div className="provider-empty-hint">
              No keys configured. Add one below to start forwarding requests.
            </div>
          )}
          {backends.map((b) => (
            <BackendRow
              key={b.id}
              backend={b}
              healthStatus={healthMap.get(b.name)}
              onDelete={() => onDeleteBackend(b)}
            />
          ))}
          <AddBackendForm
            provider={provider}
            existingCount={backends.length}
            onCreated={() => {}}
          />
        </div>
      </div>
    </>
  )
}

// ── Main Providers Tab ─────────────────────────────────────────────────────────

export default function Providers() {
  const catalogQuery = useCatalogProviders()
  const managedQuery = useManagedBackends()
  const { data: backends } = useBackends()
  const deleteBackend = useDeleteManagedBackend()

  const [expandedId, setExpandedId] = useState<string | null>(null)
  const [search, setSearch] = useState('')
  const [pendingDelete, setPendingDelete] = useState<ManagedBackend | null>(null)

  const catalog = useMemo(() => catalogQuery.data ?? [], [catalogQuery.data])
  const managed = useMemo(() => managedQuery.data?.backends ?? [], [managedQuery.data])

  // Backend count per provider_id
  const backendsByProvider = useMemo(() => {
    const m = new Map<string, ManagedBackend[]>()
    for (const mb of managed) {
      if (!m.has(mb.provider_id)) m.set(mb.provider_id, [])
      m.get(mb.provider_id)!.push(mb)
    }
    return m
  }, [managed])

  // Health lookup: backend name -> status
  // useBackends() returns {backends: Backend[]} despite the type annotation
  const healthMap = useMemo(() => {
    const m = new Map<string, string>()
    const list = Array.isArray(backends) ? backends : (backends as unknown as { backends: { name: string; status: string }[] })?.backends ?? []
    for (const b of list) m.set(b.name, b.status)
    return m
  }, [backends])

  // Filter by search, then group by tier
  const filtered = useMemo(() => {
    const lc = search.toLowerCase()
    const matching = lc
      ? catalog.filter((p) => p.display_name.toLowerCase().includes(lc) || p.id.includes(lc))
      : catalog
    return groupByTier(matching)
  }, [catalog, search])

  const expandedProvider = expandedId ? catalog.find((p) => p.id === expandedId) : null
  const expandedBackends = expandedId ? backendsByProvider.get(expandedId) ?? [] : []

  function doDelete() {
    if (!pendingDelete) return Promise.resolve()
    return deleteBackend.mutateAsync(pendingDelete.name).then(() => undefined)
  }

  return (
    <div>
      <div className="section-header">
        <h2>Providers</h2>
        <input
          type="search"
          name="provider-search"
          placeholder="Search providers..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          style={{ width: 260 }}
        />
      </div>

      <AsyncBoundary
        query={catalogQuery}
        errorTitle="Failed to load provider catalog"
        empty={{
          when: () => catalog.length === 0,
          render: () => (
            <div className="empty-cta">
              <div className="empty-cta-title">No providers available</div>
              <div className="empty-cta-body">
                The provider catalog is empty. Check that the providers crate is loaded.
              </div>
            </div>
          ),
        }}
      >
        {() => (
          <div className="provider-catalog">
            {filtered.map((group) => (
              <div key={group.tier}>
                <div className="provider-tier-label">{group.label}</div>
                <div className={`provider-tile-grid${group.tier === 0 ? ' tier-top' : ''}`}>
                  {group.providers.map((p) => (
                    <ProviderTile
                      key={p.id}
                      provider={p}
                      backendCount={backendsByProvider.get(p.id)?.length ?? 0}
                      onClick={() => setExpandedId(p.id)}
                    />
                  ))}
                </div>
              </div>
            ))}
            {filtered.length === 0 && search && (
              <div className="dim" style={{ padding: 20 }}>
                No providers match "{search}".
              </div>
            )}
          </div>
        )}
      </AsyncBoundary>

      {expandedProvider && (
        <ProviderDetailPanel
          key={expandedId}
          provider={expandedProvider}
          backends={expandedBackends}
          healthMap={healthMap}
          onClose={() => setExpandedId(null)}
          onDeleteBackend={setPendingDelete}
        />
      )}

      <ConfirmDialog
        open={pendingDelete !== null}
        onClose={() => setPendingDelete(null)}
        onConfirm={doDelete}
        title="Delete backend?"
        message={
          <>
            Delete backend <span className="mono">{pendingDelete?.name}</span>? Routes
            referencing this backend will lose it from their provider list.
          </>
        }
      />
    </div>
  )
}
