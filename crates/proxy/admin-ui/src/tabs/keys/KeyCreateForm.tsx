import { useState } from 'react'
import { useCreateKey } from '../../api/queries'
import { AdminButton } from '../../components/shared/Performative'
import { buildCreateKeyPayload } from './keyPayload'

export default function KeyCreateForm({ onCreated }: { onCreated: (key: string) => void }) {
  const create = useCreateKey()
  const [desc, setDesc] = useState('')
  const [spendLimit, setSpendLimit] = useState('')
  const [rpmLimit, setRpmLimit] = useState('')

  function handleSubmit() {
    create.mutate(buildCreateKeyPayload({
      description: desc,
      spendLimit,
      rpmLimit,
    }), {
      onSuccess: (res) => {
        setDesc(''); setSpendLimit(''); setRpmLimit('')
        onCreated(res.key)
      },
    })
  }

  return (
    <div className="form-group">
      <div className="form-label">Create Key</div>
      <form onSubmit={(e) => { e.preventDefault(); handleSubmit() }}>
        <div className="form-row" style={{ flexWrap: 'wrap' }}>
          <input name="description" placeholder="Description" value={desc} onChange={(e) => setDesc(e.target.value)} />
          <input name="max_budget_usd" placeholder="Spend limit USD" type="number" value={spendLimit} onChange={(e) => setSpendLimit(e.target.value)} style={{ width: 160 }} />
          <input name="rpm_limit" placeholder="RPM limit" type="number" value={rpmLimit} onChange={(e) => setRpmLimit(e.target.value)} style={{ width: 100 }} />
          <AdminButton type="submit" tone="primary" loading={create.isPending}>
            Create
          </AdminButton>
        </div>
        {!spendLimit && !rpmLimit && (
          <div className="form-hint" style={{ color: 'var(--warn)', marginTop: 6 }}>
            No limits set — this key will be unrestricted (unlimited spend and requests).
          </div>
        )}
      </form>
    </div>
  )
}
