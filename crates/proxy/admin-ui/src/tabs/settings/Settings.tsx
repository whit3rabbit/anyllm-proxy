import { Fragment, useState } from 'react'
import { useConfig, useSaveConfig, useDeleteConfigOverride, useEnv } from '../../api/queries'
import EmptyState from '../../components/shared/EmptyState'

export default function Settings() {
  const { data: cfg, isLoading, error } = useConfig()
  const { data: envData } = useEnv()
  const save = useSaveConfig()
  const del = useDeleteConfigOverride()
  const [form, setForm] = useState<Record<string, string>>({})

  function handleSave(key: string) {
    const val = form[key]
    if (val === undefined) return
    save.mutate({ [key]: val })
  }

  return (
    <div>
      <EmptyState loading={isLoading} error={error?.message} />
      {cfg && (
        <div>
          {cfg.entries.map((entry) => (
            <div className="form-group" key={entry.key}>
              <div className="form-label">{entry.key}</div>
              <div className="form-row">
                <input
                  value={form[entry.key] ?? entry.value}
                  onChange={(e) => setForm((f) => ({ ...f, [entry.key]: e.target.value }))}
                />
                <button className="btn btn-primary btn-sm" onClick={() => handleSave(entry.key)}>Save</button>
                <button className="btn btn-secondary btn-sm" onClick={() => del.mutate(entry.key)}>Reset</button>
              </div>
            </div>
          ))}
        </div>
      )}
      {envData && (
        <div className="readonly-section" style={{ marginTop: 16 }}>
          <div className="section-label">Environment</div>
          <div style={{ display: 'grid', gridTemplateColumns: '220px 1fr', gap: '4px 12px', marginTop: 8, fontSize: 12 }}>
            {Object.entries(envData).map(([k, v]) => (
              <Fragment key={k}>
                <span className="dim">{k}</span>
                <span className="mono">{v}</span>
              </Fragment>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}
