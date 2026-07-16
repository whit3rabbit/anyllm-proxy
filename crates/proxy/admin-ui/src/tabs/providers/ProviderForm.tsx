import { useEffect, useState } from 'react'
import {
  useCreateManagedBackend,
  useUpdateManagedBackend,
  useDiscoverModels,
} from '../../api/queries'
import type { CatalogProvider, ManagedBackend } from '../../api/types'
import { getProviderFields, resolveDiscoveryUrl } from '../../utils/providerFields'
import { AdminButton } from '../../components/shared/Performative'

// A provider is "local" when its default endpoint is a loopback address.
function isLocalProvider(p: CatalogProvider): boolean {
  return /localhost|127\.0\.0\.1|0\.0\.0\.0/.test(p.default_base_url ?? '')
}

function errorMessage(err: Error | null, fallback: string): string {
  if (!err) return fallback
  try {
    const parsed = JSON.parse(err.message)
    if (parsed && typeof parsed.error === 'string') return parsed.error
  } catch {
    /* not JSON */
  }
  return err.message || fallback
}

/**
 * Create or edit a managed backend for a given provider. Rendered inside the
 * provider detail Modal (large, single column). On edit, non-secret fields are
 * pre-seeded and resent on save: ManagedBackendPatch has no null sentinel, so
 * omitting a field would silently keep the stale value.
 */
export default function ProviderForm({
  provider,
  existing,
  existingCount,
  onDone,
}: {
  provider: CatalogProvider
  existing?: ManagedBackend | null
  existingCount: number
  onDone?: () => void
}) {
  const isEdit = !!existing
  const create = useCreateManagedBackend()
  const update = useUpdateManagedBackend()
  const discover = useDiscoverModels()

  const initialForm = (): Record<string, string> => {
    if (existing) {
      const seeded: Record<string, string> = { name: existing.name }
      for (const k of ['api_base', 'deployment', 'api_version', 'project', 'region'] as const) {
        if (existing[k] != null) seeded[k] = String(existing[k])
      }
      if (existing.rpm != null) seeded.rpm = String(existing.rpm)
      if (existing.tpm != null) seeded.tpm = String(existing.tpm)
      return seeded
    }
    const base: Record<string, string> = { name: `${provider.id}-${existingCount + 1}` }
    if (isLocalProvider(provider) && provider.default_base_url) {
      base.api_base = provider.default_base_url
    }
    return base
  }

  const [form, setForm] = useState<Record<string, string>>(initialForm)
  const [shown, setShown] = useState<Record<string, boolean>>({})
  const [models, setModels] = useState<string[]>([])
  const [modelInput, setModelInput] = useState('')

  useEffect(() => {
    setForm(initialForm())
    setModels([])
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [provider.id, existing?.name])

  const fields = getProviderFields(provider)

  function set(name: string, value: string) {
    setForm(p => ({ ...p, [name]: value }))
  }

  function addModel(name: string) {
    const m = name.trim()
    if (m && !models.includes(m)) setModels(p => [...p, m])
    setModelInput('')
  }

  function queryModels() {
    // Local providers relax SSRF (loopback/LAN) based on provider_id.
    discover.mutate(
      {
        source: 'custom',
        url: form.api_base || provider.default_base_url,
        provider_id: provider.id,
        api_key: form.api_key || undefined,
      },
      {
        onSuccess: data => {
          setModels(prev => {
            const merged = new Set(prev)
            for (const m of data.models) merged.add(m.id)
            return [...merged]
          })
        },
      },
    )
  }

  function submit() {
    const payload = {
      api_key: form.api_key || undefined,
      api_base: form.api_base ? form.api_base.trim().replace(/\/+$/, '') : undefined,
      deployment: form.deployment || undefined,
      api_version: form.api_version || undefined,
      project: form.project || undefined,
      region: form.region || undefined,
      aws_access_key_id: form.aws_access_key_id || undefined,
      aws_secret_access_key: form.aws_secret_access_key || undefined,
      aws_session_token: form.aws_session_token || undefined,
      rpm: form.rpm ? Number(form.rpm) : undefined,
      tpm: form.tpm ? Number(form.tpm) : undefined,
    }
    if (isEdit && existing) {
      update.mutate({ name: existing.name, data: payload }, { onSuccess: () => onDone?.() })
    } else {
      create.mutate(
        { name: form.name, provider_id: provider.id, ...payload },
        {
          onSuccess: () => {
            setForm(initialForm())
            setModels([])
            onDone?.()
          },
        },
      )
    }
  }

  function credSet(fieldName: string): boolean {
    if (!existing) return false
    if (fieldName === 'api_key') return existing.api_key_set
    if (fieldName === 'aws_access_key_id' || fieldName === 'aws_secret_access_key')
      return existing.aws_creds_set
    return false
  }

  const pending = create.isPending || update.isPending
  const submitError = create.error || update.error
  // The name input's `pattern` attr never fires (no <form>/submit event), so
  // validate here to match what the backend enforces before sending.
  const nameValid = isEdit || /^[a-zA-Z0-9_-]+$/.test(form.name ?? '')

  return (
    <div className="provider-add-form">
      <div className="form-group">
        <label className="form-label" htmlFor="pf-name">Name</label>
        <input
          id="pf-name"
          type="text"
          value={form.name ?? ''}
          disabled={isEdit}
          pattern="[a-zA-Z0-9_\-]+"
          onChange={e => set('name', e.target.value)}
          style={{ width: '100%' }}
        />
        {!nameValid && form.name && (
          <div className="form-hint" style={{ color: 'var(--danger, #d33)' }}>
            Name may contain only letters, numbers, hyphens, and underscores.
          </div>
        )}
      </div>

      {fields.map(f => {
        const isPassword = f.type === 'password'
        const inputType = isPassword && !shown[f.name] ? 'password' : f.type === 'url' ? 'text' : f.type
        return (
          <div key={f.name} className="form-group">
            <label className="form-label" htmlFor={`pf-${f.name}`}>{f.label}</label>
            {f.hint && <div className="form-hint">{f.hint}</div>}
            <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
              <input
                id={`pf-${f.name}`}
                type={inputType}
                placeholder={credSet(f.name) ? '••••••••' : f.placeholder}
                value={form[f.name] ?? ''}
                onChange={e => set(f.name, e.target.value)}
                style={{ flex: 1 }}
              />
              {isPassword && (
                <AdminButton
                  size="sm"
                  type="button"
                  onClick={() => setShown(s => ({ ...s, [f.name]: !s[f.name] }))}
                >
                  {shown[f.name] ? 'Hide' : 'Show'}
                </AdminButton>
              )}
            </div>
            {f.name === 'api_base' && (() => {
              const target = resolveDiscoveryUrl(form.api_base || provider.default_base_url || '')
              if (!target) return null
              const unsupported = ['vertex_ai', 'gemini_native', 'bedrock_native'].includes(
                provider.protocol,
              )
              return (
                <div className="form-hint">
                  Query models will request: <span className="mono">{target}</span>
                  {unsupported && ' — model discovery may not work for this provider.'}
                </div>
              )
            })()}
          </div>
        )
      })}

      {/* Models (informational): discovered or hand-added names. Not persisted
          on the backend today; used to sanity-check connectivity. */}
      <div className="form-group">
        <label className="form-label" htmlFor="pf-model">Models</label>
        <div style={{ display: 'flex', gap: 6 }}>
          <input
            id="pf-model"
            type="text"
            placeholder="Enter model name and press Enter to add"
            value={modelInput}
            onChange={e => setModelInput(e.target.value)}
            onKeyDown={e => {
              if (e.key === 'Enter') {
                e.preventDefault()
                addModel(modelInput)
              }
            }}
            style={{ flex: 1 }}
          />
          <AdminButton size="sm" type="button" onClick={() => addModel(modelInput)}>
            Add Model
          </AdminButton>
        </div>
        {models.length > 0 && (
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, marginTop: 8 }}>
            {models.map(m => (
              <span key={m} className="badge-cap active" style={{ cursor: 'pointer' }}
                title="Remove"
                onClick={() => setModels(prev => prev.filter(x => x !== m))}>
                {m} ✕
              </span>
            ))}
          </div>
        )}
        {discover.isError && (
          <div className="inline-error">{errorMessage(discover.error, 'Failed to query models')}</div>
        )}
      </div>

      {submitError && (
        <div className="inline-error">{errorMessage(submitError, 'Failed to save backend')}</div>
      )}

      <div className="provider-add-actions">
        <AdminButton
          size="sm"
          type="button"
          onClick={queryModels}
          disabled={discover.isPending || (!form.api_base && !provider.default_base_url)}
          loading={discover.isPending}
        >
          Query models
        </AdminButton>
        <AdminButton
          tone="primary"
          size="sm"
          type="button"
          onClick={submit}
          disabled={(!isEdit && (!form.name || !nameValid)) || pending}
          loading={pending}
        >
          {isEdit ? 'Save' : 'Create'}
        </AdminButton>
      </div>
    </div>
  )
}
