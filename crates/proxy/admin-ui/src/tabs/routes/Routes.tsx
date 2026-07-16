import { useState } from 'react'
import {
  useRoutes,
  useCreateRoute,
  useUpdateRoute,
  useDeleteRoute,
  useRouteProviders,
  useAddRouteProvider,
  useUpdateRouteProvider,
  useRemoveRouteProvider,
  useReorderRouteProviders,
  useManagedBackends,
  useUpdateManagedBackend,
  useCatalogProviders,
  useCatalogProviderModels,
  useStatus,
} from '../../api/queries'
import type { Route, ManagedBackend, CatalogProvider } from '../../api/types'
import AsyncBoundary from '../../components/shared/AsyncBoundary'
import Modal from '../../components/shared/Modal'
import ConfirmDialog from '../../components/shared/ConfirmDialog'
import { AdminButton, AdminLoading, AdminSurface } from '../../components/shared/Performative'
import ProviderForm from '../providers/ProviderForm'
import { catalogModelIds } from '../../utils/catalogModels'
import { copyToClipboard } from '../../utils/clipboard'
import { pushToast } from '../../store/toast'

// ── Route Detail (expanded inline) ────────────────────────────────────────────

const STRATEGIES = ['failover', 'round-robin', 'least-busy', 'latency', 'weighted', 'cost']

/** inherit (null) / on (true) / off (false) selector for a nullable bool override. */
function TriStateSelect({
  value,
  onChange,
}: {
  value: boolean | null
  onChange: (v: boolean | null) => void
}) {
  const str = value === null ? 'inherit' : value ? 'on' : 'off'
  return (
    <select
      value={str}
      onChange={(e) => {
        const v = e.target.value
        onChange(v === 'inherit' ? null : v === 'on')
      }}
    >
      <option value="inherit">inherit (global)</option>
      <option value="on">on</option>
      <option value="off">off</option>
    </select>
  )
}

/**
 * Component displaying details of a specific route, including its mapped providers,
 * reordering priority controls, limits, and code snippets.
 */
function RouteDetail({ route, onClose }: { route: Route; onClose: () => void }) {
  const { data: providersData, isLoading } = useRouteProviders(route.id)
  const addProvider = useAddRouteProvider()
  const updateProvider = useUpdateRouteProvider()
  const removeProvider = useRemoveRouteProvider()
  const reorder = useReorderRouteProviders()
  const updateRoute = useUpdateRoute()
  const updateBackend = useUpdateManagedBackend()
  const { data: managedData } = useManagedBackends()
  const { data: catalogProviders } = useCatalogProviders()
  const { data: status } = useStatus()
  const [showAdd, setShowAdd] = useState(false)
  const [selectedBackend, setSelectedBackend] = useState('')
  const [selectedModel, setSelectedModel] = useState('*')
  const [editingBackend, setEditingBackend] = useState<ManagedBackend | null>(null)

  const providers = providersData?.providers ?? []
  const backends = managedData?.backends ?? []

  // Model dropdown for the provider chosen in the add form. Models aren't stored
  // on a backend; they come from the provider's catalog + live-cached ids.
  const selectedProviderId = backends.find((b) => b.id === selectedBackend)?.provider_id ?? null
  const modelsQuery = useCatalogProviderModels(selectedProviderId)
  const modelOptions = catalogModelIds(modelsQuery.data)
  const catalogProviderFor = (providerId: string): CatalogProvider | undefined =>
    catalogProviders?.find((p) => p.id === providerId)

  // Routes have no unique URL; the proxy dispatches by the `model` field (= route
  // name). Show a ready-to-run curl snippet against the shared endpoint. Host comes
  // from how the admin UI was reached; port from the proxy status.
  const proxyUrl = `http://${window.location.hostname}:${status?.proxy_port ?? 3000}/v1/chat/completions`
  const curlSnippet = [
    `curl ${proxyUrl} \\`,
    `  -H 'Authorization: Bearer <key>' \\`,
    `  -H 'Content-Type: application/json' \\`,
    `  -d '${JSON.stringify({ model: route.name, messages: [{ role: 'user', content: 'hi' }] })}'`,
  ].join('\n')

  /** Copies the Curl code snippet representing the route to the clipboard. */
  async function copyCurl() {
    const ok = await copyToClipboard(curlSnippet)
    pushToast(
      ok
        ? { variant: 'success', message: 'curl snippet copied' }
        : { variant: 'error', message: 'Copy failed (clipboard blocked)' },
    )
  }
  const usedBackendIds = new Set(providers.map((p) => p.backend_id))
  const availableBackends = backends.filter((b) => !usedBackendIds.has(b.id))

  function handleAdd() {
    if (!selectedBackend) return
    const models = selectedModel === '*' ? ['*'] : [selectedModel]
    addProvider.mutate(
      { routeId: route.id, data: { backend_id: selectedBackend, models, priority: providers.length, enabled: true } },
      { onSuccess: () => { setShowAdd(false); setSelectedBackend(''); setSelectedModel('*') } },
    )
  }

  function move(idx: number, delta: -1 | 1) {
    const target = idx + delta
    if (target < 0 || target >= providers.length) return
    const next = providers.slice()
    const [row] = next.splice(idx, 1)
    next.splice(target, 0, row)
    reorder.mutate({ routeId: route.id, data: { provider_ids: next.map((p) => p.id) } })
  }

  return (
    <div className="route-detail">
      <div className="route-detail-header">
        <div>
          <span className="route-detail-title">{route.name}</span>
          {route.description && <span className="dim route-detail-desc">{route.description}</span>}
        </div>
        <div className="route-detail-meta">
          {route.rpm && <span className="dim mono">RPM {route.rpm}</span>}
          <AdminButton
            size="sm"
            tone={route.enabled ? 'primary' : 'secondary'}
            title="Route on/off. Disabled routes stop dispatching and lose virtual-key scope."
            onClick={() => updateRoute.mutate({ id: route.id, data: { enabled: !route.enabled } })}
          >
            {route.enabled ? 'route on' : 'route off'}
          </AdminButton>
          <AdminButton size="sm" onClick={onClose}>Close</AdminButton>
        </div>
      </div>

      {route.enabled && (
        <div className="route-detail-curl">
          <div className="route-detail-curl-head">
            <span className="section-label">Call this route</span>
            <AdminButton size="sm" onClick={copyCurl}>Copy curl</AdminButton>
          </div>
          <pre className="route-detail-curl-body mono">{curlSnippet}</pre>
          <div className="dim route-detail-curl-hint">
            The route is selected by the <code>model</code> field (= route name). Replace{' '}
            <code>&lt;key&gt;</code> with a proxy or virtual key.
          </div>
        </div>
      )}

      <div className="route-detail-options">
        <span className="section-label route-detail-subhead-label">Route options</span>
        <div className="route-options-grid">
          <label className="route-option">
            <span className="dim">Strategy</span>
            <select
              value={route.strategy}
              onChange={(e) => updateRoute.mutate({ id: route.id, data: { strategy: e.target.value } })}
            >
              {STRATEGIES.map((s) => <option key={s} value={s}>{s}</option>)}
            </select>
          </label>
          <label className="route-option">
            <span className="dim">Position (lower wins across routes)</span>
            <input
              type="number"
              name="route-position"
              defaultValue={route.position}
              onBlur={(e) => {
                const v = Number.parseInt(e.target.value, 10)
                if (Number.isNaN(v) || v === route.position) return
                updateRoute.mutate({ id: route.id, data: { position: v } })
              }}
            />
          </label>
          <label className="route-option">
            <span className="dim">Guardrails</span>
            <select
              value={route.guardrail_mode ?? 'inherit'}
              onChange={(e) =>
                updateRoute.mutate({
                  id: route.id,
                  data: { guardrail_mode: e.target.value === 'inherit' ? null : e.target.value },
                })
              }
            >
              <option value="inherit">inherit (global)</option>
              <option value="disabled">disabled</option>
              <option value="standard">standard</option>
            </select>
          </label>
          <label className="route-option">
            <span className="dim">Secret redaction</span>
            <TriStateSelect
              value={route.redact_secrets}
              onChange={(v) => updateRoute.mutate({ id: route.id, data: { redact_secrets: v } })}
            />
          </label>
          <label className="route-option">
            <span className="dim">Image compression</span>
            <TriStateSelect
              value={route.pxpipe_compress}
              onChange={(v) => updateRoute.mutate({ id: route.id, data: { pxpipe_compress: v } })}
            />
          </label>
          <label className="route-option route-option-wide">
            <span className="dim">Compression model scope (CSV, blank = inherit)</span>
            <input
              type="text"
              name="route-pxpipe-models"
              defaultValue={route.pxpipe_models ?? ''}
              placeholder="inherit global"
              onBlur={(e) => {
                const v = e.target.value.trim()
                if ((route.pxpipe_models ?? '') === v) return
                updateRoute.mutate({ id: route.id, data: { pxpipe_models: v === '' ? null : v } })
              }}
            />
          </label>
        </div>
        <div className="dim route-options-note">
          Overrides apply only where the feature already runs (image compression: Anthropic passthrough
          backends only). "inherit" / blank uses the global value from Settings.
        </div>
      </div>

      <div className="route-detail-subhead">
        <span className="section-label route-detail-subhead-label">Providers (priority order)</span>
        <AdminButton tone="primary" size="sm" onClick={() => setShowAdd(!showAdd)}>
          {showAdd ? 'Cancel' : '+ Add Provider'}
        </AdminButton>
      </div>

      {showAdd && (
        <div className="route-detail-add">
          <select
            value={selectedBackend}
            onChange={(e) => { setSelectedBackend(e.target.value); setSelectedModel('*') }}
            className="route-detail-add-select"
          >
            <option value="">Select provider...</option>
            {availableBackends.map((b) => (
              <option key={b.id} value={b.id}>{b.name} ({b.provider_id})</option>
            ))}
          </select>
          <select
            value={selectedModel}
            onChange={(e) => setSelectedModel(e.target.value)}
            className="route-detail-add-models"
            disabled={!selectedBackend}
          >
            <option value="*">* (all models)</option>
            {modelOptions.map((m) => (
              <option key={m} value={m}>{m}</option>
            ))}
          </select>
          <AdminButton
            tone="primary"
            size="sm"
            onClick={handleAdd}
            disabled={!selectedBackend || addProvider.isPending}
            loading={addProvider.isPending}
          >
            Add
          </AdminButton>
          {selectedBackend && !modelsQuery.isLoading && modelOptions.length === 0 && (
            <span className="dim route-detail-add-hint">
              No models found for this provider. Add one in the Providers tab (or use <code>*</code> for all).
            </span>
          )}
        </div>
      )}

      {isLoading && <div className="dim"><AdminLoading label="Loading providers" /></div>}

      {!isLoading && providers.length === 0 && (
        <div className="dim route-detail-empty">No providers assigned. Click "+ Add Provider" above.</div>
      )}

      {!isLoading && providers.map((p, idx) => (
        <div key={p.id} className="route-provider-row">
          <span className="dim mono">{idx + 1}.</span>
          <span>
            <span className="route-provider-name">{p.backend_name}</span>
            <span className="dim route-provider-id">({p.provider_id})</span>
          </span>
          <span className="mono dim route-provider-models">[{p.models.join(', ')}]</span>
          <span className="route-provider-reorder">
            <AdminButton
              tone="icon"
              size="sm"
              className="btn-icon"
              onClick={() => move(idx, -1)}
              disabled={idx === 0 || reorder.isPending}
              aria-label="Move up"
            >&uarr;</AdminButton>
            <AdminButton
              tone="icon"
              size="sm"
              className="btn-icon"
              onClick={() => move(idx, 1)}
              disabled={idx >= providers.length - 1 || reorder.isPending}
              aria-label="Move down"
            >&darr;</AdminButton>
          </span>
          <span className="route-provider-actions">
            <AdminButton
              size="sm"
              tone={p.enabled ? 'primary' : 'secondary'}
              className="route-provider-toggle"
              title="In-route membership: whether this backend is active within this route."
              onClick={() => updateProvider.mutate({ routeId: route.id, providerId: p.id, data: { enabled: !p.enabled } })}
            >
              {p.enabled ? 'in route' : 'excluded'}
            </AdminButton>
            {(() => {
              const backend = backends.find((b) => b.id === p.backend_id)
              if (!backend) return null
              return (
                <AdminButton
                  size="sm"
                  tone={backend.enabled ? 'primary' : 'secondary'}
                  className="route-provider-toggle"
                  title="Backend online (global). Disables this backend everywhere, not just this route."
                  onClick={() => updateBackend.mutate({ name: backend.name, data: { enabled: !backend.enabled } })}
                >
                  {backend.enabled ? 'backend on' : 'backend off'}
                </AdminButton>
              )
            })()}
            {(() => {
              const backend = backends.find((b) => b.id === p.backend_id)
              // Only editable if we can resolve both the backend record and its
              // catalog provider (custom providers have no CatalogProvider).
              if (!backend || !catalogProviderFor(p.provider_id)) return null
              return (
                <AdminButton
                  size="sm"
                  className="route-provider-toggle"
                  title="Edit this provider's credentials and settings."
                  onClick={() => setEditingBackend(backend)}
                >
                  Edit
                </AdminButton>
              )
            })()}
            <AdminButton
              tone="danger"
              size="sm"
              className="route-provider-remove"
              onClick={() => removeProvider.mutate({ routeId: route.id, providerId: p.id })}
            >
              Remove
            </AdminButton>
          </span>
        </div>
      ))}

      {editingBackend && catalogProviderFor(editingBackend.provider_id) && (
        <Modal
          open
          onClose={() => setEditingBackend(null)}
          title={`Edit provider — ${editingBackend.name}`}
          size="lg"
        >
          <ProviderForm
            provider={catalogProviderFor(editingBackend.provider_id)!}
            existing={editingBackend}
            siblingNames={backends
              .filter((b) => b.provider_id === editingBackend.provider_id)
              .map((b) => b.name)}
            onDone={() => setEditingBackend(null)}
          />
        </Modal>
      )}
    </div>
  )
}

// ── Create Route Modal ────────────────────────────────────────────────────────

/**
 * Modal dialog component for creating a new route.
 */
function CreateRouteModal({ onClose }: { onClose: () => void }) {
  const create = useCreateRoute()
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [strategy, setStrategy] = useState('failover')

  function submit() {
    create.mutate(
      { name, description: description || undefined, strategy },
      { onSuccess: onClose },
    )
  }

  return (
    <Modal
      open
      onClose={onClose}
      title="New Route"
      size="sm"
      dismissable={!create.isPending}
      footer={
        <>
          <AdminButton onClick={onClose} disabled={create.isPending}>Cancel</AdminButton>
          <AdminButton tone="primary" onClick={submit} disabled={!name.trim() || create.isPending} loading={create.isPending}>
            Create
          </AdminButton>
        </>
      }
    >
      <div className="form-group">
        <label className="form-label" htmlFor="route-name">Name</label>
        <input
          id="route-name"
          name="name"
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="e.g. default, cheap"
          style={{ width: '100%' }}
        />
      </div>
      <div className="form-group">
        <label className="form-label" htmlFor="route-desc">Description</label>
        <input
          id="route-desc"
          name="description"
          type="text"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder="optional"
          style={{ width: '100%' }}
        />
      </div>
      <div className="form-group">
        <label className="form-label" htmlFor="route-strategy">Strategy</label>
        <select
          id="route-strategy"
          name="strategy"
          value={strategy}
          onChange={(e) => setStrategy(e.target.value)}
          style={{ width: '100%' }}
        >
          {STRATEGIES.map((s) => <option key={s} value={s}>{s}</option>)}
        </select>
      </div>
      <div className="dim" style={{ fontSize: '0.85em' }}>
        Per-route options (guardrails, compression, secret redaction) and on/off are set after
        creation from the route's detail panel.
      </div>
      {create.isError && <div className="error">Failed to create route</div>}
    </Modal>
  )
}

// ── Main Routes Tab ───────────────────────────────────────────────────────────

/**
 * Main Routes tab component.
 * Displays list of routes, strategies, status toggles, and allows creating routes.
 */
export default function Routes() {
  const query = useRoutes()
  const deleteRoute = useDeleteRoute()
  const [expandedId, setExpandedId] = useState<string | null>(null)
  const [showCreate, setShowCreate] = useState(false)
  const [pendingDelete, setPendingDelete] = useState<Route | null>(null)

  function doDelete() {
    if (!pendingDelete) return Promise.resolve()
    return deleteRoute.mutateAsync(pendingDelete.id).then(() => undefined)
  }

  return (
    <div>
      <div className="section-header">
        <h2>Model Routes</h2>
        <AdminButton tone="primary" onClick={() => setShowCreate(true)}>+ New Route</AdminButton>
      </div>
      <p style={{ color: 'var(--text-2)', marginTop: -4, marginBottom: 14 }}>
        A route is a named model alias: the client sends <code>model: &lt;route-name&gt;</code> and the
        request is load-balanced across the route's backends by the chosen strategy (failover,
        round-robin, and so on). Distinct from Auto Router, which routes by request shape.
      </p>

      <AsyncBoundary
        query={query}
        errorTitle="Failed to load routes"
        empty={{
          when: (d) => (d.routes?.length ?? 0) === 0,
          render: () => (
            <AdminSurface className="empty-cta">
              <div className="empty-cta-title">No routes yet</div>
              <div className="empty-cta-body">
                Create a route to fan requests out across multiple backends with priority-based failover.
              </div>
              <AdminButton tone="primary" onClick={() => setShowCreate(true)}>+ New Route</AdminButton>
            </AdminSurface>
          ),
        }}
      >
        {(data) => (
          <table className="route-table">
            <thead>
              <tr>
                <th>Name</th>
                <th>Strategy</th>
                <th>Providers</th>
                <th>Limits</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {data.routes.map((r) => (
                <RouteRow
                  key={r.id}
                  route={r}
                  expanded={expandedId === r.id}
                  onToggle={() => setExpandedId(expandedId === r.id ? null : r.id)}
                  onDelete={() => setPendingDelete(r)}
                />
              ))}
            </tbody>
          </table>
        )}
      </AsyncBoundary>

      {showCreate && <CreateRouteModal onClose={() => setShowCreate(false)} />}
      <ConfirmDialog
        open={pendingDelete !== null}
        onClose={() => setPendingDelete(null)}
        onConfirm={doDelete}
        title="Delete route?"
        message={
          <>
            Delete route <span className="mono">{pendingDelete?.name}</span>? Virtual keys scoped
            to this route will lose access. This cannot be undone.
          </>
        }
      />
    </div>
  )
}

function RouteRow({
  route,
  expanded,
  onToggle,
  onDelete,
}: {
  route: Route
  expanded: boolean
  onToggle: () => void
  onDelete: () => void
}) {
  const limits = [route.rpm && `RPM ${route.rpm}`, route.tpm && `TPM ${route.tpm}`].filter(Boolean).join(', ') || '—'

  return (
    <>
      <tr className="route-row" onClick={onToggle}>
        <td className="route-row-name">
          {expanded ? '\u25BE ' : '\u25B8 '}{route.name}
          {!route.enabled && <span className="dim route-row-desc">(disabled)</span>}
          {route.description && <span className="dim route-row-desc">{route.description}</span>}
        </td>
        <td className="dim">{route.strategy}</td>
        <td>{route.provider_count}</td>
        <td className="mono dim">{limits}</td>
        <td className="route-row-actions">
          <AdminButton
            tone="danger"
            size="sm"
            onClick={(e) => { e.stopPropagation(); onDelete() }}
          >
            Delete
          </AdminButton>
        </td>
      </tr>
      {expanded && (
        <tr>
          <td colSpan={5} className="route-row-detail-cell">
            <RouteDetail route={route} onClose={onToggle} />
          </td>
        </tr>
      )}
    </>
  )
}
