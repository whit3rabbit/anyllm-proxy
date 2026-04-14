import { useMemo, useState } from 'react'
import { useModels, useAddModel, useRemoveModel, useDiscoverModels, useBackends, useManagedBackends } from '../../api/queries'
import AsyncBoundary from '../../components/shared/AsyncBoundary'
import ConfirmDialog from '../../components/shared/ConfirmDialog'

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
  const [model, setModel] = useState('')
  const [provider, setProvider] = useState('openai')
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

  function doRemove() {
    if (!pendingRemove) return Promise.resolve()
    return remove.mutateAsync(pendingRemove).then(() => undefined)
  }

  const filter = search.trim().toLowerCase()
  const filteredModels = useMemo(() => {
    const all = modelsQuery.data?.models ?? []
    if (!filter) return all
    return all.filter((m) =>
      m.name.toLowerCase().includes(filter) ||
      m.model.toLowerCase().includes(filter) ||
      m.provider.toLowerCase().includes(filter),
    )
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
          <button
            className="btn btn-secondary"
            onClick={handleDiscover}
            disabled={discover.isPending || (discoverSource === 'custom' && !customUrl)}
          >
            {discover.isPending ? 'Fetching...' : 'Fetch'}
          </button>
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
                  onClick={() => setModel(m.id)}
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

      {/* Datalist for backend name suggestions */}
      <datalist id="backends-list">
        {backends?.map(b => (
          <option key={b.name} value={b.name}>{b.name}</option>
        ))}
        {managedBackends?.backends.map(b => (
          <option key={`managed-${b.name}`} value={b.name}>{b.name} (managed)</option>
        ))}
      </datalist>

      {/* Manual add model form */}
      <div className="form-group">
        <div className="form-label">Add Model</div>
        <div className="form-row" style={{ flexWrap: 'wrap' }}>
          <input name="model-name" placeholder="Virtual name" value={name} onChange={(e) => setName(e.target.value)} />
          <input name="model-id" placeholder="Model ID" value={model} onChange={(e) => setModel(e.target.value)} />
          <select name="provider" value={provider} onChange={(e) => setProvider(e.target.value)}>
            <option value="openai">openai</option>
            <option value="anthropic">anthropic</option>
            <option value="gemini">gemini</option>
            <option value="vertex">vertex</option>
            <option value="azure">azure</option>
            <option value="bedrock">bedrock</option>
          </select>
          <input
            name="backend"
            placeholder="Backend (optional)"
            value={backendName}
            onChange={(e) => setBackendName(e.target.value)}
            list="backends-list"
          />
          <button
            className="btn btn-primary"
            onClick={() => add.mutate({ name, model, provider, ...(backendName ? { backend_name: backendName } : {}) })}
            disabled={!name || !model || add.isPending}
          >
            Add
          </button>
        </div>
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
            <div className="empty-cta">
              <div className="empty-cta-title">No models configured</div>
              <div className="empty-cta-body">
                Add a model above, or use Discover to pull a catalog from OpenRouter, DeepInfra, Ollama, or a custom endpoint.
              </div>
            </div>
          ),
        }}
      >
        {(data) =>
          filteredModels.length === 0 ? (
            <div className="empty">No models match "{search}".</div>
          ) : (
            <table className="route-table">
              <thead>
                <tr><th>Virtual Name</th><th>Model</th><th>Provider</th><th>Strategy</th><th></th></tr>
              </thead>
              <tbody>
                {filteredModels.map((m) => (
                  <tr key={`${m.name}-${m.model}`}>
                    <td className="mono">{m.name}</td>
                    <td className="mono">{m.model}</td>
                    <td className="dim">{m.provider}</td>
                    <td className="dim">{data.routing_strategy}</td>
                    <td>
                      <button className="btn btn-danger btn-sm" onClick={() => setPendingRemove(m.name)}>Remove</button>
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
