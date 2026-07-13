import { useEffect, useMemo, useState } from 'react'
import {
  useCatalogProviders,
  useManagedBackends,
  useCreateManagedBackend,
  useDeleteManagedBackend,
  useUptime,
  useFavorites,
  useToggleFavorite,
  useDiscoverModels,
} from '../../api/queries'
import type { CatalogProvider, ManagedBackend } from '../../api/types'
import { getProviderFields, resolveDiscoveryUrl } from '../../utils/providerFields'
import { groupSections } from '../../utils/providerTiers'

// A provider is "local" when its default endpoint is a loopback address.
// Used to pre-fill the endpoint field so local-LLM users don't type it.
function isLocalProvider(p: CatalogProvider): boolean {
  return /localhost|127\.0\.0\.1|0\.0\.0\.0/.test(p.default_base_url ?? '')
}

// Backend errors arrive as `{"error":"..."}` JSON in the thrown Error's message.
// Surface the real message instead of a generic banner.
function errorMessage(err: Error | null, fallback: string): string {
  if (!err) return fallback
  try {
    const parsed = JSON.parse(err.message)
    if (parsed && typeof parsed.error === 'string') return parsed.error
  } catch {
    // not JSON; fall through to the raw message
  }
  return err.message || fallback
}
import AsyncBoundary from '../../components/shared/AsyncBoundary'
import ConfirmDialog from '../../components/shared/ConfirmDialog'
import Modal from '../../components/shared/Modal'
import StatusDot from '../../components/shared/StatusDot'
import ProviderIcon from '../../components/shared/ProviderIcon'
import { AdminButton, AdminSurface } from '../../components/shared/Performative'

// ── Provider Tile ──────────────────────────────────────────────────────────────

/**
 * Component representing a provider tile in the grid.
 * Displays provider details, configured credentials count, and favorite state.
 */
function ProviderTile({
  provider,
  backendCount,
  favorited,
  onToggleFavorite,
  onClick,
}: {
  provider: CatalogProvider
  backendCount: number
  favorited: boolean
  onToggleFavorite: () => void
  onClick: () => void
}) {
  // Heart is a sibling, not a child, of the tile button — nesting <button> is invalid DOM.
  return (
    <div className="provider-tile-wrap">
      <button
        type="button"
        className={`provider-tile${backendCount > 0 ? ' has-backends' : ''}`}
        onClick={onClick}
      >
        <ProviderIcon id={provider.id} size={28} />
        <span className="provider-tile-name">{provider.display_name}</span>
        <span className="provider-tile-id">{provider.id}</span>
        {backendCount > 0 && (
          <span className="provider-tile-count">
            {backendCount} key{backendCount !== 1 ? 's' : ''}
          </span>
        )}
      </button>
      <button
        type="button"
        className={`provider-fav-btn${favorited ? ' active' : ''}`}
        aria-label={favorited ? 'Remove from favorites' : 'Add to favorites'}
        aria-pressed={favorited}
        title={favorited ? 'Unfavorite' : 'Favorite'}
        onClick={(e) => {
          e.stopPropagation()
          onToggleFavorite()
        }}
      >
        {favorited ? '♥' : '♡'}
      </button>
    </div>
  )
}

// ── Backend Row (inside detail panel) ──────────────────────────────────────────

/**
 * Renders a row for a configured managed backend credentials instance.
 */
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
        status={healthStatus === 'up' ? 'ok' : healthStatus === 'down' ? 'err' : 'dim'}
        pulse={healthStatus === 'up'}
      />
      <span className="backend-name">{backend.name}</span>
      <span className="backend-status">
        {backend.api_key_set ? 'key set' : 'no key'}
        {backend.rpm != null && <> &middot; RPM {backend.rpm}</>}
      </span>
      <AdminButton tone="danger" size="sm" onClick={onDelete}>
        Delete
      </AdminButton>
    </div>
  )
}

// ── Add Backend Form (inside detail panel) ─────────────────────────────────────

/**
 * Component providing fields to configure credentials and create a new managed backend.
 */
function AddBackendForm({
  provider,
  existingCount,
}: {
  provider: CatalogProvider
  existingCount: number
}) {
  const create = useCreateManagedBackend()
  const discover = useDiscoverModels()
  // Local providers get their loopback endpoint pre-filled (editable); hosted ones don't,
  // so we never send a redundant api_base for them.
  const initialForm = (): Record<string, string> => {
    const base: Record<string, string> = {
      name: `${provider.id}-${existingCount + 1}`,
      provider_id: provider.id,
    }
    if (isLocalProvider(provider) && provider.default_base_url) {
      base.api_base = provider.default_base_url
    }
    return base
  }
  const [form, setForm] = useState<Record<string, string>>(initialForm)

  // Reset form when the provider or count changes
  useEffect(() => {
    setForm(initialForm())
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [provider.id, existingCount])

  function submit() {
    create.mutate(
      {
        name: form.name,
        provider_id: provider.id,
        api_key: form.api_key || undefined,
        api_base: form.api_base ? form.api_base.trim().replace(/\/+$/, '') : undefined,
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
        // useCreateManagedBackend invalidates ['managed-backends'], so the list refreshes itself.
        // Reset the form so a second key can be added without stale field values.
        onSuccess: () => setForm(initialForm()),
      },
    )
  }

  function queryModels() {
    // For local providers the backend relaxes SSRF (loopback/LAN) based on provider_id.
    discover.mutate({
      source: 'custom',
      url: form.api_base || provider.default_base_url,
      provider_id: provider.id,
      api_key: form.api_key || undefined,
    })
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
          {f.name === 'api_base' && (() => {
            const target = resolveDiscoveryUrl(form.api_base || provider.default_base_url || '')
            if (!target) return null
            // /v1/models discovery only works for OpenAI-shaped and Anthropic-native providers.
            const unsupported = ['vertex_ai', 'gemini_native', 'bedrock_native'].includes(
              provider.protocol,
            )
            return (
              <div className="form-hint">
                Query models will request: <span className="mono">{target}</span>
                {unsupported && ' — model discovery may not work for this provider.'}
              </div>
            )
          })()}
        </div>
      ))}
      {discover.isError && (
        <div className="inline-error">{errorMessage(discover.error, 'Failed to query models')}</div>
      )}
      {discover.isSuccess && (
        <div className="form-hint">
          {discover.data.models.length > 0
            ? `Found ${discover.data.models.length} model(s): ${discover.data.models
                .slice(0, 8)
                .map((m) => m.id)
                .join(', ')}${discover.data.models.length > 8 ? ', …' : ''}`
            : 'No models returned by the server.'}
        </div>
      )}
      {create.isError && (
        <div className="inline-error">{errorMessage(create.error, 'Failed to create backend')}</div>
      )}
      <div className="provider-add-actions">
        <AdminButton
          size="sm"
          onClick={() => setForm(initialForm())}
          disabled={create.isPending}
        >
          Reset
        </AdminButton>
        <AdminButton
          size="sm"
          onClick={queryModels}
          disabled={discover.isPending || (!form.api_base && !provider.default_base_url)}
          loading={discover.isPending}
        >
          Query models
        </AdminButton>
        <AdminButton
          tone="primary"
          size="sm"
          onClick={submit}
          disabled={!form.name || create.isPending}
          loading={create.isPending}
        >
          Create
        </AdminButton>
      </div>
    </div>
  )
}

// ── Provider Detail Panel (lightbox overlay) ───────────────────────────────────

/**
 * Modal overlay displaying provider details, capabilities, and configured managed backends.
 */
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
    <Modal open onClose={onClose} title={`${provider.display_name} (${provider.id})`} size="md">
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
        />
      </div>
    </Modal>
  )
}

// ── Main Providers Tab ─────────────────────────────────────────────────────────

/**
 * Main Providers tab component.
 * Displays available LiteLLM providers grouped by tier, allows favoriting, and manages backend keys.
 */
export default function Providers() {
  const catalogQuery = useCatalogProviders()
  const managedQuery = useManagedBackends()
  const { data: uptime } = useUptime()
  const { data: favorites } = useFavorites()
  const toggleFavorite = useToggleFavorite()
  const deleteBackend = useDeleteManagedBackend()

  const favoriteIds = useMemo(() => new Set(favorites ?? []), [favorites])

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

  // Health lookup: backend name -> status. Real per-backend health lives in the
  // uptime endpoint (health_checks table), not get_backends.
  const healthMap = useMemo(() => {
    const m = new Map<string, string>()
    for (const b of uptime?.backends ?? []) m.set(b.name, b.status)
    return m
  }, [uptime])

  // Filter by search, then group by tier
  const filtered = useMemo(() => {
    const lc = search.toLowerCase()
    const matching = lc
      ? catalog.filter((p) => p.display_name.toLowerCase().includes(lc) || p.id.includes(lc))
      : catalog
    return groupSections(matching, favoriteIds)
  }, [catalog, search, favoriteIds])

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
            <AdminSurface className="empty-cta">
              <div className="empty-cta-title">No providers available</div>
              <div className="empty-cta-body">
                The provider catalog is empty. Check that the providers crate is loaded.
              </div>
            </AdminSurface>
          ),
        }}
      >
        {() => (
          <div className="provider-catalog">
            {filtered.map((group) => (
              <div key={group.key}>
                <div className="provider-tier-label">{group.label}</div>
                <div className={`provider-tile-grid${group.top ? ' tier-top' : ''}`}>
                  {group.providers.map((p) => (
                    <ProviderTile
                      key={p.id}
                      provider={p}
                      backendCount={backendsByProvider.get(p.id)?.length ?? 0}
                      favorited={favoriteIds.has(p.id)}
                      onToggleFavorite={() =>
                        toggleFavorite.mutate({ providerId: p.id, on: !favoriteIds.has(p.id) })
                      }
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
