import { useState } from 'react'
import { useModels, useAddModel, useRemoveModel, useDiscoverModels, useBackends, useManagedBackends } from '../../api/queries'
import EmptyState from '../../components/shared/EmptyState'

const AUTH_HINTS: Record<string, { text: string; needsKey: boolean }> = {
  openrouter: { text: 'Public, no key needed', needsKey: false },
  deepinfra: { text: 'Public, no key needed', needsKey: false },
  ollama: { text: 'No key needed (local)', needsKey: false },
  configured: { text: 'API key required', needsKey: true },
  custom: { text: 'API key may be required', needsKey: true },
}

// Inline SVG key icon (12x12), used as auth indicator next to sources that need a key.
function KeyIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 16 16" fill="none" style={{ verticalAlign: '-1px', marginRight: 3 }}>
      <path
        d="M10.5 1a4.5 4.5 0 0 0-4.1 6.35L2 11.75V15h3.25v-2H7v-1.75h1.75L9.65 10.4A4.5 4.5 0 1 0 10.5 1zm1 3a1 1 0 1 1 0-2 1 1 0 0 1 0 2z"
        fill="currentColor"
      />
    </svg>
  )
}

export default function Models() {
  const { data, isLoading, error } = useModels()
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

  const hint = AUTH_HINTS[discoverSource] ?? AUTH_HINTS.custom

  function handleDiscover() {
    discover.mutate({
      source: discoverSource,
      ...(discoverSource === 'custom' ? { url: customUrl } : {}),
    })
  }


  return (
    <div>
      {/* Discover models section */}
      <div style={{ marginBottom: 20 }}>
        <div className="section-label" style={{ marginBottom: 8 }}>Discover Models</div>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
          <select value={discoverSource} onChange={(e) => { setDiscoverSource(e.target.value); discover.reset() }}>
            <option value="openrouter">OpenRouter</option>
            <option value="deepinfra">DeepInfra</option>
            <option value="ollama">Ollama (local)</option>
            <option value="configured">Configured backend</option>
            <option value="custom">Custom URL</option>
          </select>
          {discoverSource === 'custom' && (
            <input
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
          <span className="dim" style={{ fontSize: 12 }}>
            {hint.needsKey && <KeyIcon />}{hint.text}
          </span>
        </div>

        {/* Discovery error */}
        {discover.isError && (
          <div style={{ marginTop: 8, padding: '6px 10px', background: 'var(--err-dim)', borderLeft: '3px solid var(--err)', borderRadius: 'var(--r)', fontSize: 12 }}>
            {discover.error.message}
          </div>
        )}

        {/* Discovery results */}
        {discover.data && discover.data.models.length > 0 && (
          <div style={{ marginTop: 8 }}>
            <div className="dim" style={{ fontSize: 12, marginBottom: 4 }}>
              {discover.data.models.length} model{discover.data.models.length !== 1 ? 's' : ''} found.
              Click to populate the form below.
            </div>
            <div style={{ maxHeight: 200, overflowY: 'auto', border: '1px solid var(--border)', borderRadius: 'var(--r)', fontSize: 12 }}>
              {discover.data.models.map((m) => (
                <div
                  key={m.id}
                  onClick={() => setModel(m.id)}
                  style={{
                    padding: '4px 8px',
                    cursor: 'pointer',
                    borderBottom: '1px solid var(--border)',
                    background: model === m.id ? 'var(--accent-dim)' : undefined,
                  }}
                  onMouseEnter={(e) => { (e.target as HTMLElement).style.background = 'var(--surface-2)' }}
                  onMouseLeave={(e) => { (e.target as HTMLElement).style.background = model === m.id ? 'var(--accent-dim)' : '' }}
                >
                  <span className="mono">{m.id}</span>
                  {m.name && m.name !== m.id && <span className="dim" style={{ marginLeft: 8 }}>{m.name}</span>}
                </div>
              ))}
            </div>
          </div>
        )}

        {discover.data && discover.data.models.length === 0 && (
          <div className="dim" style={{ marginTop: 8, fontSize: 12 }}>No models returned.</div>
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
          <input placeholder="Virtual name" value={name} onChange={(e) => setName(e.target.value)} />
          <input placeholder="Model ID" value={model} onChange={(e) => setModel(e.target.value)} />
          <select value={provider} onChange={(e) => setProvider(e.target.value)}>
            <option value="openai">openai</option>
            <option value="anthropic">anthropic</option>
            <option value="gemini">gemini</option>
            <option value="vertex">vertex</option>
            <option value="azure">azure</option>
            <option value="bedrock">bedrock</option>
          </select>
          <input
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
      <EmptyState loading={isLoading} error={error?.message} />
      {data && (
        <table className="route-table">
          <thead>
            <tr><th>Virtual Name</th><th>Model</th><th>Provider</th><th>Strategy</th><th></th></tr>
          </thead>
          <tbody>
            {data.models.map((m) => (
              <tr key={`${m.name}-${m.model}`}>
                <td className="mono">{m.name}</td>
                <td className="mono">{m.model}</td>
                <td className="dim">{m.provider}</td>
                <td className="dim">{data.routing_strategy}</td>
                <td>
                  <button className="btn btn-danger btn-sm" onClick={() => remove.mutate(m.name)}>Remove</button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  )
}
