import { useState } from 'react'
import { useCreateKey } from '../../api/queries'
import { AdminButton } from '../../components/shared/Performative'

export default function KeyCreateForm({ onCreated }: { onCreated: (key: string) => void }) {
  const create = useCreateKey()
  const [desc, setDesc] = useState('')
  const [spendLimit, setSpendLimit] = useState('')
  const [rpmLimit, setRpmLimit] = useState('')

  function handleSubmit() {
    create.mutate({
      description: desc || null,
      spend_limit: spendLimit ? Number(spendLimit) : null,
      rpm_limit: rpmLimit ? Number(rpmLimit) : null,
    }, {
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
          <input name="spend_limit" placeholder="Spend limit USD" type="number" value={spendLimit} onChange={(e) => setSpendLimit(e.target.value)} style={{ width: 120 }} />
          <input name="rpm_limit" placeholder="RPM limit" type="number" value={rpmLimit} onChange={(e) => setRpmLimit(e.target.value)} style={{ width: 100 }} />
          <AdminButton type="submit" tone="primary" loading={create.isPending}>
            Create
          </AdminButton>
        </div>
      </form>
    </div>
  )
}
