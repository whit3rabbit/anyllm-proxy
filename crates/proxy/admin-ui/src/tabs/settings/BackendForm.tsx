import { useState, useEffect } from 'react'
import {
  useCatalogProviders,
  useCreateManagedBackend,
  useUpdateManagedBackend,
} from '../../api/queries'
import type { ManagedBackend, CatalogProvider } from '../../api/types'
import { getProviderFields } from '../../utils/providerFields'
import ProviderIcon from '../../components/shared/ProviderIcon'
import { AdminButton } from '../../components/shared/Performative'

interface BackendFormProps {
  initial?: ManagedBackend | null
  onSuccess: () => void
  onCancel: () => void
}

export function BackendForm({ initial, onSuccess, onCancel }: BackendFormProps) {
  const isEdit = !!initial
  const { data: providers = [] } = useCatalogProviders()
  const create = useCreateManagedBackend()
  const update = useUpdateManagedBackend()

  const [name, setName] = useState(initial?.name ?? '')
  const [providerId, setProviderId] = useState(initial?.provider_id ?? '')
  // Seed non-secret fields from the existing backend so an edit shows current
  // values. PUT omits empty fields and the patch has no clear-to-NULL sentinel,
  // so a blank form would either no-op or look like it wiped the endpoint config.
  // Secrets (api_key, aws_*) are never returned; leave them blank (placeholder shows •••).
  const [fields, setFields] = useState<Record<string, string>>(() => {
    if (!initial) return {}
    const seeded: Record<string, string> = {}
    for (const k of ['api_base', 'deployment', 'api_version', 'project', 'region'] as const) {
      if (initial[k] != null) seeded[k] = initial[k] as string
    }
    if (initial.rpm != null) seeded.rpm = String(initial.rpm)
    if (initial.tpm != null) seeded.tpm = String(initial.tpm)
    return seeded
  })
  const [submitError, setSubmitError] = useState<string | null>(null)

  useEffect(() => {
    if (providers.length > 0 && !providerId) {
      setProviderId(initial?.provider_id ?? providers[0].id)
    }
  }, [providers.length]) // eslint-disable-line react-hooks/exhaustive-deps

  const selectedProvider: CatalogProvider | undefined = providers.find(p => p.id === providerId)
    ?? (providers.length > 0 ? providers[0] : undefined)

  const fieldDefs = selectedProvider ? getProviderFields(selectedProvider) : []

  const authFields = fieldDefs.filter(f => f.group === 'auth')
  const endpointFields = fieldDefs.filter(f => f.group === 'endpoint')
  const limitFields = fieldDefs.filter(f => f.group === 'limits')

  function getFieldValue(fieldName: string): string {
    return fields[fieldName] ?? ''
  }

  function setFieldValue(fieldName: string, value: string) {
    setFields(prev => ({ ...prev, [fieldName]: value }))
  }

  function isCredSetForField(fieldName: string): boolean {
    if (!isEdit || !initial) return false
    if (fieldName === 'api_key') return initial.api_key_set
    if (fieldName === 'aws_secret_access_key' || fieldName === 'aws_access_key_id') return initial.aws_creds_set
    return false
  }

  function omitEmpty(obj: Record<string, string>): Record<string, string> {
    return Object.fromEntries(Object.entries(obj).filter(([, v]) => v !== ''))
  }

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    setSubmitError(null)

    const cleanedFields = omitEmpty(fields)

    if (isEdit && initial) {
      update.mutate(
        { name: initial.name, data: cleanedFields },
        {
          onSuccess: () => onSuccess(),
          onError: (err) => setSubmitError(err.message),
        },
      )
    } else {
      if (!name) {
        setSubmitError('Name is required')
        return
      }
      create.mutate(
        { name, provider_id: providerId, ...cleanedFields } as Parameters<typeof create.mutate>[0],
        {
          onSuccess: () => onSuccess(),
          onError: (err) => setSubmitError(err.message),
        },
      )
    }
  }

  const isPending = create.isPending || update.isPending

  function renderField(fd: ReturnType<typeof getProviderFields>[0]) {
    const val = getFieldValue(fd.name)
    const credSet = isCredSetForField(fd.name)
    const inputType = fd.type === 'url' ? 'text' : fd.type

    return (
      <div key={fd.name} style={{ marginBottom: 10 }}>
        <div style={{ fontSize: 12, color: 'var(--text-2)', marginBottom: 3 }}>
          {fd.label}{fd.required && <span style={{ color: 'var(--err)', marginLeft: 2 }}>*</span>}
        </div>
        <input
          type={inputType}
          value={val}
          placeholder={credSet ? '••••••••' : fd.placeholder}
          onChange={(e) => setFieldValue(fd.name, e.target.value)}
          style={{ width: '100%' }}
        />
        {fd.hint && (
          <div style={{ fontSize: 11, color: 'var(--text-2)', marginTop: 3 }}>{fd.hint}</div>
        )}
      </div>
    )
  }

  return (
    <form onSubmit={handleSubmit} style={{ padding: '12px', background: 'var(--bg-raised)', border: '1px solid var(--border)', borderRadius: 'var(--rm)', marginBottom: 14 }}>
      <div style={{ fontWeight: 600, marginBottom: 12, fontSize: 13 }}>
        {isEdit ? `Edit backend: ${initial!.name}` : 'Add managed backend'}
      </div>

      {/* Provider selector */}
      <div style={{ marginBottom: 10 }}>
        <div style={{ fontSize: 12, color: 'var(--text-2)', marginBottom: 3 }}>
          Provider<span style={{ color: 'var(--err)', marginLeft: 2 }}>*</span>
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          {providerId && <ProviderIcon id={providerId} size={18} style={{ flexShrink: 0 }} />}
          <select
            value={providerId}
            onChange={(e) => { setProviderId(e.target.value); setFields({}) }}
            disabled={isEdit}
            style={{ flex: 1 }}
          >
          {providers.length === 0 && (
            <option value="">Loading providers…</option>
          )}
          {(['implemented', 'wired', 'stub'] as const).map(status => {
            const group = providers.filter(p => p.status === status)
            if (group.length === 0) return null
            return (
              <optgroup key={status} label={status.charAt(0).toUpperCase() + status.slice(1)}>
                {group.map(p => (
                  <option key={p.id} value={p.id}>{p.display_name} ({p.id})</option>
                ))}
              </optgroup>
            )
          })}
          </select>
        </div>
      </div>

      <div style={{ marginBottom: 10 }}>
        <div style={{ fontSize: 12, color: 'var(--text-2)', marginBottom: 3 }}>
          Name<span style={{ color: 'var(--err)', marginLeft: 2 }}>*</span>
        </div>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          required
          pattern="[a-zA-Z0-9_\-]+"
          placeholder="my-backend"
          disabled={isEdit}
          style={{ width: '100%' }}
        />
        {!isEdit && (
          <div style={{ fontSize: 11, color: 'var(--text-2)', marginTop: 3 }}>Letters, numbers, underscores, hyphens only</div>
        )}
      </div>

      {authFields.length > 0 && (
        <div style={{ marginBottom: 4 }}>
          <div className="section-label" style={{ marginBottom: 6 }}>Authentication</div>
          {authFields.map(renderField)}
        </div>
      )}

      {endpointFields.length > 0 && (
        <div style={{ marginBottom: 4 }}>
          <div className="section-label" style={{ marginBottom: 6 }}>Endpoint</div>
          {endpointFields.map(renderField)}
        </div>
      )}

      {limitFields.length > 0 && (
        <details style={{ marginBottom: 10 }}>
          <summary style={{ fontSize: 11, color: 'var(--text-2)', cursor: 'pointer', textTransform: 'uppercase', letterSpacing: '0.07em', fontWeight: 500, marginBottom: 6 }}>
            Rate Limits
          </summary>
          <div style={{ marginTop: 8 }}>
            {limitFields.map(renderField)}
          </div>
        </details>
      )}

      {/* Error */}
      {submitError && (
        <div style={{ marginBottom: 10, padding: '6px 10px', background: 'var(--err-dim)', borderLeft: '3px solid var(--err)', borderRadius: 'var(--r)', fontSize: 12 }}>
          {submitError}
        </div>
      )}

      {/* Actions */}
      <div style={{ display: 'flex', gap: 8 }}>
        <AdminButton type="submit" tone="primary" size="sm" loading={isPending}>
          {isEdit ? 'Save changes' : 'Create backend'}
        </AdminButton>
        <AdminButton type="button" size="sm" onClick={onCancel} disabled={isPending}>
          Cancel
        </AdminButton>
      </div>
    </form>
  )
}
