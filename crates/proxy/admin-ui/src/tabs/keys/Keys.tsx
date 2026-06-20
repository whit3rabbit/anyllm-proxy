import { useEffect, useMemo, useState } from 'react'
import { useSearchParams } from 'react-router-dom'
import { useKeys } from '../../api/queries'
import AsyncBoundary from '../../components/shared/AsyncBoundary'
import Badge from '../../components/shared/Badge'
import BudgetBar from '../../components/shared/BudgetBar'
import KeyCreateForm from './KeyCreateForm'
import KeyEditModal from './KeyEditModal'
import type { VirtualKey } from '../../api/types'

export default function Keys() {
  const query = useKeys()
  const [params, setParams] = useSearchParams()
  const search = params.get('q') ?? ''
  const editId = params.get('edit')
  const [newKey, setNewKey] = useState<string | null>(null)
  const [editing, setEditing] = useState<VirtualKey | null>(null)

  useEffect(() => {
    if (!editId || !query.data) return
    const match = query.data.find((k) => String(k.id) === editId)
    if (match) setEditing(match)
  }, [editId, query.data])

  function closeEdit() {
    setEditing(null)
    if (params.has('edit')) {
      const next = new URLSearchParams(params)
      next.delete('edit')
      setParams(next, { replace: true })
    }
  }

  function openEdit(k: VirtualKey) {
    setEditing(k)
    const next = new URLSearchParams(params)
    next.set('edit', String(k.id))
    setParams(next, { replace: true })
  }

  function setSearch(value: string) {
    const next = new URLSearchParams(params)
    if (value) next.set('q', value); else next.delete('q')
    setParams(next, { replace: true })
  }

  const filter = search.trim().toLowerCase()
  const filtered = useMemo(() => {
    if (!filter) return query.data ?? []
    return (query.data ?? []).filter((k) => {
      const desc = (k.description ?? '').toLowerCase()
      const prefix = k.key_prefix.toLowerCase()
      return desc.includes(filter) || prefix.includes(filter)
    })
  }, [query.data, filter])

  return (
    <div>
      <KeyCreateForm onCreated={setNewKey} />
      {newKey && (
        <div className="key-result">
          <div className="key-result-label">New key (copy now — not shown again)</div>
          {newKey}
        </div>
      )}
      <div className="toolbar">
        <input
          type="search"
          name="keys-search"
          placeholder="Search by description or prefix…"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="toolbar-search"
        />
        {query.data && (
          <span className="dim toolbar-count">
            {filtered.length} of {query.data.length}
          </span>
        )}
      </div>
      <AsyncBoundary
        query={query}
        errorTitle="Failed to load keys"
        empty={{
          when: (keys) => keys.length === 0,
          render: () => (
            <div className="empty-cta">
              <div className="empty-cta-title">No virtual keys yet</div>
              <div className="empty-cta-body">
                Use the form above to create one. Keys are shown once at creation and hashed in storage.
              </div>
            </div>
          ),
        }}
      >
        {() =>
          filtered.length === 0 ? (
            <div className="empty">No keys match "{search}".</div>
          ) : (
            <table className="keys-grid">
              <thead>
                <tr>
                  <th>Prefix</th><th>Description</th><th>Status</th>
                  <th>Spend</th><th>Requests</th><th>Created</th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((k) => (
                  <tr key={k.id} style={{ cursor: 'pointer' }} onClick={() => openEdit(k)}>
                    <td className="mono">{k.key_prefix}…</td>
                    <td className="dim">{k.description ?? '—'}</td>
                    <td><Badge variant={k.status} /></td>
                    <td><BudgetBar spent={k.period_spend_usd} limit={k.max_budget_usd} /></td>
                    <td className="mono">{k.total_requests.toLocaleString()}</td>
                    <td className="mono dim">{k.created_at.slice(0, 10)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )
        }
      </AsyncBoundary>
      {editing && <KeyEditModal key={editing.id} vk={editing} onClose={closeEdit} />}
    </div>
  )
}
