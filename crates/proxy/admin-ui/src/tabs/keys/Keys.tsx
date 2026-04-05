import { useState } from 'react'
import { useKeys } from '../../api/queries'
import Badge from '../../components/shared/Badge'
import BudgetBar from '../../components/shared/BudgetBar'
import EmptyState from '../../components/shared/EmptyState'
import KeyCreateForm from './KeyCreateForm'
import KeyEditModal from './KeyEditModal'
import type { VirtualKey } from '../../api/types'

export default function Keys() {
  const { data, isLoading, error } = useKeys()
  const [newKey, setNewKey] = useState<string | null>(null)
  const [editing, setEditing] = useState<VirtualKey | null>(null)

  return (
    <div>
      <KeyCreateForm onCreated={setNewKey} />
      {newKey && (
        <div className="key-result">
          <div className="key-result-label">New key (copy now — not shown again)</div>
          {newKey}
        </div>
      )}
      <EmptyState loading={isLoading} error={error?.message} empty={data?.length === 0} message="No keys" />
      {data && data.length > 0 && (
        <table className="keys-grid">
          <thead>
            <tr>
              <th>Prefix</th><th>Description</th><th>Status</th>
              <th>Spend</th><th>Requests</th><th>Created</th>
            </tr>
          </thead>
          <tbody>
            {data.map((k) => (
              <tr key={k.id} style={{ cursor: 'pointer' }} onClick={() => setEditing(k)}>
                <td className="mono">{k.key_prefix}…</td>
                <td className="dim">{k.description ?? '—'}</td>
                <td><Badge variant={k.status} /></td>
                <td><BudgetBar spent={k.total_spend} limit={k.spend_limit} /></td>
                <td className="mono">{k.total_requests.toLocaleString()}</td>
                <td className="mono dim">{k.created_at.slice(0, 10)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {editing && <KeyEditModal key={editing.id} vk={editing} onClose={() => setEditing(null)} />}
    </div>
  )
}
