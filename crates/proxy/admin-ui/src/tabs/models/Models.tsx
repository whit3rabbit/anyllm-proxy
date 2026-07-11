import { useMemo, useState } from 'react'
import { useModels, useAddModel, useRemoveModel, useDiscoverModels, useBackends, useManagedBackends } from '../../api/queries'
import AsyncBoundary from '../../components/shared/AsyncBoundary'
import ConfirmDialog from '../../components/shared/ConfirmDialog'
import { AdminButton, AdminSurface } from '../../components/shared/Performative'

const AUTH_HINTS: Record<string, { text: string; needsKey: boolean }> = {
  openrouter: { text: 'Public, no key needed', needsKey: false },
  deepinfra: { text: 'Public, no key needed', needsKey: false },
  ollama: { text: 'No key needed (local)', needsKey: false },
  configured: { text: 'API key required', needsKey: true },
  custom: { text: 'API key may be required', needsKey: true },
}

function KeyIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 16 16" fill="none" className="key-icon-inline">
      <path
        d="M10.5 1a4.5 4.5 0 0 0-4.1 6.35L2 11.75V15h3.25v-2H7v-1.75h1.75L9.65 10.4A4.5 4.5 0 1 0 10.5 1zm1 3a1 1 0 1 1 0-2 1 1 0 0 1 0 2z"
        fill="currentColor"
      />
    </svg>
  )
}

export default function Models() {
  const modelsQuery = useModels()
  const add = useAddModel()
  const remove = useRemoveModel()
  const discover = useDiscoverModels()
  const { data: backends } = useBackends()
  const { data: managedBackends } = useManagedBackends()
  const [name, setName] = useState('')
  // Last value we auto-filled into Virtual Name from a discovered model. Lets us
  // re-prefill when the user picks a different model, without clobbering a name
  // they typed by hand (name !== autoName => they edited it, leave it alone).
  const [autoName, setAutoName] = useState('')
  const [model, setModel] = useState('')
  const [backendName, setBackendName] = useState('')
  const [discoverSource, setDiscoverSource] = useState('openrouter')
  const [customUrl, setCustomUrl] = useState('')
  const [search, setSearch] = useState('')
  const [pendingRemove, setPendingRemove] = useState<string | null>(null)

  const hint = AUTH_HINTS[discoverSource] ?? AUTH_HINTS.custom

  function handleDiscover() {
    discover.mutate({
      source: discoverSource,
      ...(discoverSource === 'custom' ? { url: customUrl } : {}),
    })
  }

  function handleAdd() {
    add.mutate(
      { model_name: name, actual_model: model, backend_name: backendName },
      { onSuccess: () => { setName(''); setAutoName(''); setModel(''); setBackendName('') } },
    )
  }

  function doRemove() {
    if (!pendingRemove) return Promise.resolve()
    return remove.mutateAsync(pendingRemove).then(() => undefined)
  }

  const filter = search.trim().toLowerCase()
  const filteredModels = useMemo(() => {
    const all = modelsQuery.data?.models ?? []
    if (!filter) return all
    return all.filter((m) => m.model_name.toLowerCase().includes(filter))
  }, [modelsQuery.data, filter])

  return (
    <div>
      {/* Discover models section */}
      <div className="models-discover">
        <div className="section-label">Discover Models</div>
        <div className="models-discover-row">
          <select value={discoverSource} onChange={(e) => { setDiscoverSource(e.target.value); discover.reset() }}>
            <option value="openrouter">OpenRouter</option>
            <option value="deepinfra">DeepInfra</option>
            <option value="ollama">Ollama (local)</option>
            <option value="configured">Configured backend</option>
            <option value="custom">Custom URL</option>
          </select>
          {discoverSource === 'custom' && (
            <input
              name="discover-url"
              placeholder="https://api.example.com"
              value={customUrl}
              onChange={(e) => setCustomUrl(e.target.value)}
              style={{ minWidth: 220 }}
            />
          )}
          <AdminButton
            onClick={handleDiscover}
            disabled={discover.isPending || (discoverSource === 'custom' && !customUrl)}
            loading={discover.isPending}
          >
            Fetch
          </AdminButton>
          <span className="dim models-discover-hint">
            {hint.needsKey && <KeyIcon />}{hint.text}
          </span>
        </div>

        {discover.isError && (
          <div className="inline-error">
            {discover.error.message}
          </div>
        )}

        {discover.data && discover.data.models.length > 0 && (
          <div className="models-discover-results">
            <div className="dim models-discover-count">
              {discover.data.models.length} model{discover.data.models.length !== 1 ? 's' : ''} found.
              Click to populate the form below.
            </div>
            <div className="models-discover-list">
              {discover.data.models.map((m) => (
                <div
                  key={m.id}
                  onClick={() => {
                    setModel(m.id)
                    // Prefill Virtual Name from the catalog display name (still editable).
                    // Overwrite only if empty or still holding a prior auto-fill; keep a
                    // hand-typed name.
                    if (!name || name === autoName) {
                      const next = m.name && m.name !== m.id ? m.name : m.id
                      setName(next)
                      setAutoName(next)
                    }
                  }}
                  className={`models-discover-item${model === m.id ? ' is-selected' : ''}`}
                >
                  <span className="mono">{m.id}</span>
                  {m.name && m.name !== m.id && <span className="dim models-discover-item-name">{m.name}</span>}
                </div>
              ))}
            </div>
          </div>
        )}

        {discover.data && discover.data.models.length === 0 && (
          <div className="dim models-discover-count">No models returned.</div>
        )}
      </div>

      {/* Manual add model form */}
      <div className="form-group">
        <div className="form-label">Add Model</div>
        <div className="form-row" style={{ flexWrap: 'wrap' }}>
          <input name="model-name" placeholder="Virtual name" value={name} onChange={(e) => setName(e.target.value)} />
          <input name="model-id" placeholder="Model ID" value={model} onChange={(e) => setModel(e.target.value)} />
          <select
            name="backend"
            value={backendName}
            onChange={(e) => setBackendName(e.target.value)}
          >
            <option value="">Backend…</option>
            {backends?.map(b => (
              <option key={b.name} value={b.name}>{b.name}</option>
            ))}
            {managedBackends?.backends.map(b => (
              <option key={`managed-${b.name}`} value={b.name}>{b.name} (managed)</option>
            ))}
          </select>
          <AdminButton
            tone="primary"
            onClick={handleAdd}
            disabled={!name || !model || !backendName || add.isPending}
            loading={add.isPending}
          >
            Add
          </AdminButton>
        </div>
        {add.isError && <div className="inline-error">{add.error.message}</div>}
      </div>

      <div className="toolbar">
        <input
          type="search"
          name="models-search"
          placeholder="Search models…"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="toolbar-search"
        />
        {modelsQuery.data && (
          <span className="dim toolbar-count">
            {filteredModels.length} of {modelsQuery.data.models.length}
          </span>
        )}
      </div>

      <AsyncBoundary
        query={modelsQuery}
        errorTitle="Failed to load models"
        empty={{
          when: (d) => (d.models?.length ?? 0) === 0,
          render: () => (
            <AdminSurface className="empty-cta">
              <div className="empty-cta-title">No models configured</div>
              <div className="empty-cta-body">
                Add a model above, or use Discover to pull a catalog from OpenRouter, DeepInfra, Ollama, or a custom endpoint.
              </div>
            </AdminSurface>
          ),
        }}
      >
        {(data) =>
          filteredModels.length === 0 ? (
            <div className="empty">No models match "{search}".</div>
          ) : (
            <table className="route-table">
              <thead>
                <tr><th>Virtual Name</th><th>Deployments</th><th>Strategy</th><th></th></tr>
              </thead>
              <tbody>
                {filteredModels.map((m) => (
                  <tr key={m.model_name}>
                    <td className="mono">{m.model_name}</td>
                    <td className="mono">{m.deployments}</td>
                    <td className="dim">{data.strategy ?? '—'}</td>
                    <td>
                      <AdminButton tone="danger" size="sm" onClick={() => setPendingRemove(m.model_name)}>Remove</AdminButton>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )
        }
      </AsyncBoundary>

      <ConfirmDialog
        open={pendingRemove !== null}
        onClose={() => setPendingRemove(null)}
        onConfirm={doRemove}
        title="Remove model?"
        message={
          <>
            Remove model <span className="mono">{pendingRemove}</span>? Requests using this virtual name
            will fail until another model with the same name is added.
          </>
        }
        confirmLabel="Remove"
      />
    </div>
  )
}
