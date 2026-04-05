import { useState } from 'react'
import type { VirtualKey } from '../../api/types'
import { useUpdateKey, useRevokeKey } from '../../api/queries'

interface KeyEditModalProps {
  vk: VirtualKey
  onClose: () => void
}

export default function KeyEditModal({ vk, onClose }: KeyEditModalProps) {
  const update = useUpdateKey()
  const revoke = useRevokeKey()
  const [desc, setDesc] = useState(vk.description ?? '')
  const [spendLimit, setSpendLimit] = useState(vk.spend_limit?.toString() ?? '')
  const [rpmLimit, setRpmLimit] = useState(vk.rpm_limit?.toString() ?? '')

  function handleSave() {
    update.mutate({
      id: vk.id,
      body: {
        description: desc || null,
        spend_limit: spendLimit ? Number(spendLimit) : null,
        rpm_limit: rpmLimit ? Number(rpmLimit) : null,
      },
    }, { onSuccess: onClose })
  }

  function handleRevoke() {
    if (!confirm('Revoke this key?')) return
    revoke.mutate(vk.id, { onSuccess: onClose })
  }

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-title">Edit Key — {vk.key_prefix}…</div>
        <div className="form-group">
          <div className="form-label">Description</div>
          <input value={desc} onChange={(e) => setDesc(e.target.value)} style={{ width: '100%' }} />
        </div>
        <div className="form-group">
          <div className="form-label">Spend limit (USD)</div>
          <input value={spendLimit} onChange={(e) => setSpendLimit(e.target.value)} type="number" min="0" step="0.01" />
        </div>
        <div className="form-group">
          <div className="form-label">RPM limit</div>
          <input value={rpmLimit} onChange={(e) => setRpmLimit(e.target.value)} type="number" min="0" />
        </div>
        <div className="form-row">
          <button className="btn btn-primary" onClick={handleSave}>Save</button>
          <button className="btn btn-secondary" onClick={onClose}>Cancel</button>
          <button className="btn btn-danger" style={{ marginLeft: 'auto' }} onClick={handleRevoke}>Revoke</button>
        </div>
      </div>
    </div>
  )
}
