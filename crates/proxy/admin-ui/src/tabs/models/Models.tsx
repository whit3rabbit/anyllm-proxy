import { useState } from 'react'
import { useModels, useAddModel, useRemoveModel } from '../../api/queries'
import EmptyState from '../../components/shared/EmptyState'

export default function Models() {
  const { data, isLoading, error } = useModels()
  const add = useAddModel()
  const remove = useRemoveModel()
  const [name, setName] = useState('')
  const [model, setModel] = useState('')
  const [provider, setProvider] = useState('openai')

  return (
    <div>
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
          <button
            className="btn btn-primary"
            onClick={() => add.mutate({ name, model, provider })}
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
