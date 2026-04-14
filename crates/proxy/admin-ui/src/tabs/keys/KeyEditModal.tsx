import { useState } from 'react'
import type { VirtualKey } from '../../api/types'
import { useUpdateKey, useRevokeKey, useRoutes } from '../../api/queries'
import Modal from '../../components/shared/Modal'
import ConfirmDialog from '../../components/shared/ConfirmDialog'

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
  const [selectedRoutes, setSelectedRoutes] = useState<Set<string>>(
    new Set(vk.allowed_routes ?? []),
  )
  const [confirmRevoke, setConfirmRevoke] = useState(false)
  const { data: routesData } = useRoutes()

  function handleSave() {
    update.mutate({
      id: vk.id,
      body: {
        description: desc || null,
        spend_limit: spendLimit ? Number(spendLimit) : null,
        rpm_limit: rpmLimit ? Number(rpmLimit) : null,
        allowed_routes: selectedRoutes.size > 0 ? [...selectedRoutes] : null,
      },
    }, { onSuccess: onClose })
  }

  function doRevoke() {
    return revoke.mutateAsync(vk.id).then(() => { onClose() })
  }

  return (
    <>
      <Modal
        open
        onClose={onClose}
        title={`Edit Key — ${vk.key_prefix}…`}
        dismissable={!update.isPending}
        footer={
          <>
            <button
              className="btn btn-danger"
              onClick={() => setConfirmRevoke(true)}
              disabled={update.isPending}
              style={{ marginRight: 'auto' }}
            >
              Revoke
            </button>
            <button className="btn btn-secondary" onClick={onClose} disabled={update.isPending}>Cancel</button>
            <button className="btn btn-primary" onClick={handleSave} disabled={update.isPending}>
              {update.isPending ? 'Saving…' : 'Save'}
            </button>
          </>
        }
      >
        <div className="form-group">
          <label className="form-label" htmlFor="vk-desc">Description</label>
          <input
            id="vk-desc"
            name="description"
            value={desc}
            onChange={(e) => setDesc(e.target.value)}
            style={{ width: '100%' }}
          />
        </div>
        <div className="form-group">
          <label className="form-label" htmlFor="vk-spend">Spend limit (USD)</label>
          <input
            id="vk-spend"
            name="spend_limit"
            value={spendLimit}
            onChange={(e) => setSpendLimit(e.target.value)}
            type="number"
            min="0"
            step="0.01"
          />
        </div>
        <div className="form-group">
          <label className="form-label" htmlFor="vk-rpm">RPM limit</label>
          <input
            id="vk-rpm"
            name="rpm_limit"
            value={rpmLimit}
            onChange={(e) => setRpmLimit(e.target.value)}
            type="number"
            min="0"
          />
        </div>
        <div className="form-group">
          <div className="form-label">
            Allowed routes {selectedRoutes.size === 0 && <span className="hint">(all routes)</span>}
          </div>
          <div className="route-scope-list">
            {routesData?.routes.map((r) => (
              <label key={r.id} className="route-scope-item">
                <input
                  type="checkbox"
                  name={`route-${r.id}`}
                  checked={selectedRoutes.has(r.id)}
                  onChange={() => {
                    setSelectedRoutes((prev) => {
                      const next = new Set(prev)
                      if (next.has(r.id)) next.delete(r.id)
                      else next.add(r.id)
                      return next
                    })
                  }}
                />
                <span>{r.name}</span>
              </label>
            ))}
            {(!routesData?.routes.length) && <span className="hint">No routes configured</span>}
          </div>
        </div>
      </Modal>
      <ConfirmDialog
        open={confirmRevoke}
        onClose={() => setConfirmRevoke(false)}
        onConfirm={doRevoke}
        title="Revoke key?"
        message={
          <>
            Revoking <span className="mono">{vk.key_prefix}…</span> will immediately reject
            any request using it. This cannot be undone.
          </>
        }
        confirmLabel="Revoke"
      />
    </>
  )
}
