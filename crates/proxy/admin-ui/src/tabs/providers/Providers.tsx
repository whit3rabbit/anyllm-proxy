import { useMemo, useState } from 'react'
import {
  useCatalogProviders,
  useManagedBackends,
  useCreateManagedBackend,
  useDeleteManagedBackend,
  useBackends,
} from '../../api/queries'
import type { CatalogProvider, ManagedBackend } from '../../api/types'
import { getProviderFields } from '../../utils/providerFields'
import AsyncBoundary from '../../components/shared/AsyncBoundary'
import Modal from '../../components/shared/Modal'
import ConfirmDialog from '../../components/shared/ConfirmDialog'
import StatusDot from '../../components/shared/StatusDot'

// ── Add Provider Modal ────────────────────────────────────────────────────────

function AddProviderModal({ onClose }: { onClose: () => void }) {
  const { data: catalog } = useCatalogProviders()
  const create = useCreateManagedBackend()
  const [step, setStep] = useState<'pick' | 'form'>('pick')
  const [selectedProvider, setSelectedProvider] = useState<CatalogProvider | null>(null)
  const [search, setSearch] = useState('')
  const [form, setForm] = useState<Record<string, string>>({})

  const providers = (catalog ?? [])
    .filter((p) => p.capabilities.chat_completions || p.protocol !== 'openai_compat')
    .filter((p) => !search || p.display_name.toLowerCase().includes(search.toLowerCase()))

  function pickProvider(p: CatalogProvider) {
    setSelectedProvider(p)
    setForm({ name: p.display_name.toLowerCase().replace(/\s+/g, '-'), provider_id: p.id })
    setStep('form')
  }

  function submit() {
    if (!selectedProvider) return
    create.mutate(
      {
        name: form.name,
        provider_id: selectedProvider.id,
        api_key: form.api_key || undefined,
        api_base: form.api_base || undefined,
        deployment: form.deployment || undefined,
        api_version: form.api_version || undefined,
        project: form.project || undefined,
        region: form.region || undefined,
        aws_access_key_id: form.aws_access_key_id || undefined,
        aws_secret_access_key: form.aws_secret_access_key || undefined,
        aws_session_token: form.aws_session_token || undefined,
        rpm: form.rpm ? Number(form.rpm) : undefined,
        tpm: form.tpm ? Number(form.tpm) : undefined,
      },
      { onSuccess: onClose },
    )
  }

  return (
    <Modal
      open
      onClose={onClose}
      title={step === 'pick' ? 'Add Provider' : `Configure ${selectedProvider?.display_name ?? ''}`}
      size={step === 'pick' ? 'lg' : 'md'}
      dismissable={!create.isPending}
      footer={
        step === 'form' ? (
          <>
            <button className="btn btn-secondary" onClick={() => setStep('pick')} disabled={create.isPending}>Back</button>
            <button
              className="btn btn-primary"
              onClick={submit}
              disabled={create.isPending || !form.name}
            >
              {create.isPending ? 'Saving…' : 'Create'}
            </button>
          </>
        ) : undefined
      }
    >
      {step === 'pick' && (
        <>
          <input
            type="search"
            name="provider-search"
            placeholder="Search providers..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            style={{ width: '100%', marginBottom: 12 }}
          />
          <div className="provider-pick-grid">
            {providers.map((p) => (
              <button
                key={p.id}
                type="button"
                className="provider-pick-card"
                onClick={() => pickProvider(p)}
              >
                <span className="provider-pick-name">{p.display_name}</span>
                <span className="provider-pick-proto">{p.protocol.replace(/_/g, ' ')}</span>
              </button>
            ))}
            {providers.length === 0 && (
              <div className="dim">No providers match "{search}".</div>
            )}
          </div>
        </>
      )}

      {step === 'form' && selectedProvider && (
        <>
          {getProviderFields(selectedProvider).map((f) => {
            const inputId = `prov-${f.name}`
            return (
              <div key={f.name} className="form-group">
                <label className="form-label" htmlFor={inputId}>{f.label}</label>
                {f.hint && <div className="form-hint">{f.hint}</div>}
                <input
                  id={inputId}
                  name={f.name}
                  type={f.type}
                  placeholder={f.placeholder}
                  value={form[f.name] ?? ''}
                  onChange={(e) => setForm((prev) => ({ ...prev, [f.name]: e.target.value }))}
                  style={{ width: '100%' }}
                />
              </div>
            )
          })}
          {create.isError && <div className="error">Failed to create provider</div>}
        </>
      )}
    </Modal>
  )
}

// ── Provider Card ─────────────────────────────────────────────────────────────

function ProviderCard({
  backend,
  healthStatus,
  onDelete,
}: {
  backend: ManagedBackend
  healthStatus?: string
  onDelete: (backend: ManagedBackend) => void
}) {
  return (
    <div className="card" style={{ minWidth: 200 }}>
      <div className="card-header">
        <span className="card-name">{backend.name}</span>
        <StatusDot status={healthStatus === 'ok' ? 'ok' : healthStatus ? 'err' : 'dim'} pulse={healthStatus === 'ok'} />
      </div>
      <div className="card-body">
        <div className="provider-card-meta">
          <span className="dim">Key</span>
          <span>{backend.api_key_set ? 'configured' : 'not set'}</span>
          {backend.rpm && <><span className="dim">RPM</span><span className="mono">{backend.rpm}</span></>}
          {backend.tpm && <><span className="dim">TPM</span><span className="mono">{backend.tpm}</span></>}
        </div>
        <div className="provider-card-actions">
          <button className="btn btn-danger btn-sm" onClick={() => onDelete(backend)}>Delete</button>
        </div>
      </div>
    </div>
  )
}

// ── Main Providers Tab ────────────────────────────────────────────────────────

export default function Providers() {
  const catalogQuery = useCatalogProviders()
  const managedQuery = useManagedBackends()
  const { data: backends } = useBackends()
  const deleteBackend = useDeleteManagedBackend()
  const [showAdd, setShowAdd] = useState(false)
  const [pendingDelete, setPendingDelete] = useState<ManagedBackend | null>(null)

  const catalog = catalogQuery.data
  const managed = useMemo(() => managedQuery.data?.backends ?? [], [managedQuery.data])

  // Build a health lookup: backend name → status
  const healthMap = useMemo(() => {
    const m = new Map<string, string>()
    for (const b of backends ?? []) m.set(b.name, b.status)
    return m
  }, [backends])

  // Group managed backends by provider_id
  const sortedGroups = useMemo(() => {
    const groups = new Map<string, { display: string; backends: ManagedBackend[] }>()
    for (const mb of managed) {
      const cat = catalog?.find((c) => c.id === mb.provider_id)
      const display = cat?.display_name ?? mb.provider_id
      if (!groups.has(mb.provider_id)) groups.set(mb.provider_id, { display, backends: [] })
      groups.get(mb.provider_id)!.backends.push(mb)
    }
    return [...groups.entries()].sort((a, b) => a[1].display.localeCompare(b[1].display))
  }, [managed, catalog])

  function doDelete() {
    if (!pendingDelete) return Promise.resolve()
    return deleteBackend.mutateAsync(pendingDelete.name).then(() => undefined)
  }

  return (
    <div>
      <div className="section-header">
        <h2>Providers</h2>
        <button className="btn btn-primary" onClick={() => setShowAdd(true)}>+ Add Provider</button>
      </div>

      <AsyncBoundary
        query={managedQuery}
        errorTitle="Failed to load providers"
        empty={{
          when: () => sortedGroups.length === 0,
          render: () => (
            <div className="empty-cta">
              <div className="empty-cta-title">No providers configured</div>
              <div className="empty-cta-body">
                Add a provider to start forwarding requests. Credentials are stored encrypted at rest.
              </div>
              <button className="btn btn-primary" onClick={() => setShowAdd(true)}>+ Add Provider</button>
            </div>
          ),
        }}
      >
        {() => (
          <>
            {sortedGroups.map(([providerId, group]) => (
              <div key={providerId} className="provider-group">
                <div className="section-label provider-group-label">
                  {group.display} ({group.backends.length})
                </div>
                <div className="backend-cards">
                  {group.backends.map((mb) => (
                    <ProviderCard
                      key={mb.id}
                      backend={mb}
                      healthStatus={healthMap.get(mb.name)}
                      onDelete={setPendingDelete}
                    />
                  ))}
                </div>
              </div>
            ))}
          </>
        )}
      </AsyncBoundary>

      {showAdd && <AddProviderModal onClose={() => setShowAdd(false)} />}
      <ConfirmDialog
        open={pendingDelete !== null}
        onClose={() => setPendingDelete(null)}
        onConfirm={doDelete}
        title="Delete provider?"
        message={
          <>
            Delete provider <span className="mono">{pendingDelete?.name}</span>? Routes referencing
            this backend will lose it from their provider list.
          </>
        }
      />
    </div>
  )
}
