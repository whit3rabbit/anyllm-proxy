import { useMemo, useState } from 'react'
import {
  useManagedBackends,
  useDeleteManagedBackend,
  useCatalogProviders,
} from '../../api/queries'
import type { ManagedBackend } from '../../api/types'
import EmptyState from '../../components/shared/EmptyState'
import ConfirmDialog from '../../components/shared/ConfirmDialog'
import ProviderIcon from '../../components/shared/ProviderIcon'
import { BackendForm } from './BackendForm'

type PanelState =
  | { mode: 'none' }
  | { mode: 'create' }
  | { mode: 'edit'; backend: ManagedBackend }

export function ManagedBackendsSection() {
  const { data, isLoading, error } = useManagedBackends()
  const { data: providers = [] } = useCatalogProviders()
  const del = useDeleteManagedBackend()

  const [panel, setPanel] = useState<PanelState>({ mode: 'none' })
  const [pendingDelete, setPendingDelete] = useState<ManagedBackend | null>(null)

  const providerMap = useMemo(
    () => Object.fromEntries(providers.map(p => [p.id, p.display_name])),
    [providers],
  )

  function getProviderLabel(providerId: string): string {
    return providerMap[providerId] ?? providerId
  }

  function doDelete() {
    if (!pendingDelete) return Promise.resolve()
    return del.mutateAsync(pendingDelete.name).then(() => undefined)
  }

  return (
    <div style={{ marginBottom: 24 }}>
      <div className="section-header">
        <div className="section-label" style={{ margin: 0 }}>Managed Backends</div>
        <button
          className="btn btn-primary btn-sm"
          onClick={() => setPanel({ mode: 'create' })}
        >
          Add Backend
        </button>
      </div>
      <div style={{ fontSize: 12, color: 'var(--text-2)', marginBottom: 10 }}>
        Configure provider credentials and backend settings at runtime.
      </div>

      {/* Inline form — create or edit */}
      {panel.mode === 'create' && (
        <BackendForm
          onSuccess={() => setPanel({ mode: 'none' })}
          onCancel={() => setPanel({ mode: 'none' })}
        />
      )}
      {panel.mode === 'edit' && (
        <BackendForm
          initial={panel.backend}
          onSuccess={() => setPanel({ mode: 'none' })}
          onCancel={() => setPanel({ mode: 'none' })}
        />
      )}

      <EmptyState loading={isLoading} error={error?.message} />

      {data && data.backends.length === 0 && (
        <div style={{ padding: '20px 0', color: 'var(--text-2)', fontSize: 13 }}>
          No managed backends yet. Add one to configure provider credentials at runtime.
        </div>
      )}

      {data && data.backends.length > 0 && (
        <table className="route-table">
          <thead>
            <tr>
              <th>Name</th>
              <th>Provider</th>
              <th>Credentials</th>
              <th>Base URL</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {data.backends.map((b) => (
              <tr key={b.id}>
                <td className="mono">{b.name}</td>
                <td className="dim" style={{ whiteSpace: 'nowrap' }}>
                  <ProviderIcon id={b.provider_id} size={16} style={{ marginRight: 6, verticalAlign: 'middle', opacity: 0.8 }} />
                  {getProviderLabel(b.provider_id)}
                </td>
                <td>
                  {b.api_key_set && (
                    <span className="badge badge-active" style={{ marginRight: 4 }}>Key set</span>
                  )}
                  {!b.api_key_set && !b.aws_creds_set && (
                    <span className="badge badge-revoked">No key</span>
                  )}
                  {b.aws_creds_set && (
                    <span className="badge badge-active">AWS creds set</span>
                  )}
                </td>
                <td className="dim mono" style={{ fontSize: 11 }}>{b.api_base ?? '—'}</td>
                <td>
                  <div style={{ display: 'flex', gap: 6 }}>
                    <button
                      className="btn btn-secondary btn-sm"
                      onClick={() => {
                        if (panel.mode === 'edit' && panel.backend.id === b.id) {
                          setPanel({ mode: 'none' })
                        } else {
                          setPanel({ mode: 'edit', backend: b })
                        }
                      }}
                    >
                      Edit
                    </button>
                    <button
                      className="btn btn-danger btn-sm"
                      onClick={() => setPendingDelete(b)}
                      disabled={del.isPending && del.variables === b.name}
                    >
                      Delete
                    </button>
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <ConfirmDialog
        open={pendingDelete !== null}
        onClose={() => setPendingDelete(null)}
        onConfirm={doDelete}
        title="Delete managed backend?"
        message={
          <>
            Delete backend <span className="mono">{pendingDelete?.name}</span>? Stored credentials will be
            removed. Routes still referencing it will fail until reconfigured.
          </>
        }
      />
    </div>
  )
}
