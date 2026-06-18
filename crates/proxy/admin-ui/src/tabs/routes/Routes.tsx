import { useState } from 'react'
import {
  useRoutes,
  useCreateRoute,
  useDeleteRoute,
  useRouteProviders,
  useAddRouteProvider,
  useUpdateRouteProvider,
  useRemoveRouteProvider,
  useReorderRouteProviders,
  useManagedBackends,
} from '../../api/queries'
import type { Route } from '../../api/types'
import AsyncBoundary from '../../components/shared/AsyncBoundary'
import Modal from '../../components/shared/Modal'
import ConfirmDialog from '../../components/shared/ConfirmDialog'
import { AdminButton, AdminLoading, AdminSurface } from '../../components/shared/Performative'

// ── Route Detail (expanded inline) ────────────────────────────────────────────

function RouteDetail({ route, onClose }: { route: Route; onClose: () => void }) {
  const { data: providersData, isLoading } = useRouteProviders(route.id)
  const addProvider = useAddRouteProvider()
  const updateProvider = useUpdateRouteProvider()
  const removeProvider = useRemoveRouteProvider()
  const reorder = useReorderRouteProviders()
  const { data: managedData } = useManagedBackends()
  const [showAdd, setShowAdd] = useState(false)
  const [selectedBackend, setSelectedBackend] = useState('')
  const [modelInput, setModelInput] = useState('*')

  const providers = providersData?.providers ?? []
  const backends = managedData?.backends ?? []
  const usedBackendIds = new Set(providers.map((p) => p.backend_id))
  const availableBackends = backends.filter((b) => !usedBackendIds.has(b.id))

  function handleAdd() {
    if (!selectedBackend) return
    const models = modelInput.trim() === '*' ? ['*'] : modelInput.split(',').map((s) => s.trim()).filter(Boolean)
    addProvider.mutate(
      { routeId: route.id, data: { backend_id: selectedBackend, models, priority: providers.length, enabled: true } },
      { onSuccess: () => { setShowAdd(false); setSelectedBackend(''); setModelInput('*') } },
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
          <span className="dim">strategy: {route.strategy}</span>
          {route.rpm && <span className="dim mono">RPM {route.rpm}</span>}
          <AdminButton size="sm" onClick={onClose}>Close</AdminButton>
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
          <select value={selectedBackend} onChange={(e) => setSelectedBackend(e.target.value)} className="route-detail-add-select">
            <option value="">Select provider...</option>
            {availableBackends.map((b) => (
              <option key={b.id} value={b.id}>{b.name} ({b.provider_id})</option>
            ))}
          </select>
          <input
            type="text"
            name="route-provider-models"
            placeholder="models (* for all)"
            value={modelInput}
            onChange={(e) => setModelInput(e.target.value)}
            className="route-detail-add-models"
          />
          <AdminButton
            tone="primary"
            size="sm"
            onClick={handleAdd}
            disabled={!selectedBackend || addProvider.isPending}
            loading={addProvider.isPending}
          >
            Add
          </AdminButton>
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
          <AdminButton
            size="sm"
            tone={p.enabled ? 'primary' : 'secondary'}
            className="route-provider-toggle"
            onClick={() => updateProvider.mutate({ routeId: route.id, providerId: p.id, data: { enabled: !p.enabled } })}
          >
            {p.enabled ? 'enabled' : 'disabled'}
          </AdminButton>
          <AdminButton
            tone="danger"
            size="sm"
            className="route-provider-remove"
            onClick={() => removeProvider.mutate({ routeId: route.id, providerId: p.id })}
          >
            Remove
          </AdminButton>
        </div>
      ))}
    </div>
  )
}

// ── Create Route Modal ────────────────────────────────────────────────────────

function CreateRouteModal({ onClose }: { onClose: () => void }) {
  const create = useCreateRoute()
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')

  function submit() {
    create.mutate({ name, description: description || undefined }, { onSuccess: onClose })
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
      {create.isError && <div className="error">Failed to create route</div>}
    </Modal>
  )
}

// ── Main Routes Tab ───────────────────────────────────────────────────────────

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
        <h2>Routes</h2>
        <AdminButton tone="primary" onClick={() => setShowCreate(true)}>+ New Route</AdminButton>
      </div>

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
