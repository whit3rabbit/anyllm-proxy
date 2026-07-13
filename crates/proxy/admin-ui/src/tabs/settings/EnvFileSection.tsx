import { useRef, useState } from 'react'
import { useImportEnv, downloadEnvExport } from '../../api/queries'
import { AdminButton, AdminSurface } from '../../components/shared/Performative'
import type { EnvImportResponse, EnvImportError } from '../../api/types'

const RESTART_KEY = 'env_import_pending_restart'

function restartPending() {
  return sessionStorage.getItem(RESTART_KEY) === '1'
}

export default function EnvFileSection() {
  const importEnv = useImportEnv()
  const fileRef = useRef<HTMLInputElement>(null)

  const [importResult, setImportResult] = useState<EnvImportResponse | null>(null)
  const [importError, setImportError] = useState<EnvImportError | null>(null)
  const [exportError, setExportError] = useState<string | null>(null)
  const [showRestartBanner, setShowRestartBanner] = useState(restartPending)

  function handleFileChange(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0]
    if (!file) return
    setImportResult(null)
    setImportError(null)

    importEnv.mutate(file, {
      onSuccess(data) {
        setImportResult(data)
        sessionStorage.setItem(RESTART_KEY, '1')
        setShowRestartBanner(true)
      },
      onError(err) {
        // Try to parse hard_errors from the response body
        try {
          const parsed = JSON.parse(err.message) as EnvImportError
          if (parsed.hard_errors) {
            setImportError(parsed)
            return
          }
        } catch {
          // fall through to generic error
        }
        setImportError({ hard_errors: [err.message], warnings: [] })
      },
    })

    // Reset file input so the same file can be re-selected after fixing issues
    if (fileRef.current) fileRef.current.value = ''
  }

  async function handleExport() {
    setExportError(null)
    try {
      await downloadEnvExport()
    } catch (err) {
      setExportError(err instanceof Error ? err.message : String(err))
    }
  }

  function dismissRestartBanner() {
    sessionStorage.removeItem(RESTART_KEY)
    setShowRestartBanner(false)
  }

  return (
    <div style={{ marginBottom: 24 }}>
      {/* Restart-required banner — shown after a successful import */}
      {showRestartBanner && (
        <AdminSurface className="settings-restart-banner" style={{ marginBottom: 16 }}>
          <span>Restart the proxy for imported env vars to take effect.</span>
          <AdminButton size="sm" onClick={dismissRestartBanner}>Dismiss</AdminButton>
        </AdminSurface>
      )}

      <div className="section-label" style={{ marginBottom: 8 }}>Env File</div>
      <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
        <input
          ref={fileRef}
          type="file"
          accept=".env,.anyllm.env,text/plain"
          style={{ display: 'none' }}
          onChange={handleFileChange}
        />
        <AdminButton
          size="sm"
          onClick={() => fileRef.current?.click()}
          disabled={importEnv.isPending}
          loading={importEnv.isPending}
        >
          Import .anyllm.env
        </AdminButton>
        <AdminButton size="sm" onClick={handleExport}>
          Export .anyllm.env
        </AdminButton>
      </div>

      {/* Import success */}
      {importResult && (
        <div style={{ marginTop: 10 }}>
          <div className="dim" style={{ marginBottom: 4 }}>
            {importResult.applied} variable{importResult.applied !== 1 ? 's' : ''} imported.
            {importResult.warnings.length === 0 && ' No issues.'}
          </div>
          {importResult.warnings.length > 0 && (
            <div style={{ marginTop: 8, padding: '8px 12px', background: 'var(--warn-dim)', borderLeft: '3px solid var(--warn)', borderRadius: 'var(--r)', fontSize: 12 }}>
              <div style={{ fontWeight: 600, marginBottom: 4 }}>Warnings</div>
              {importResult.warnings.map((w, i) => (
                <div key={i} className="mono" style={{ fontSize: 12 }}>
                  {w.line != null && <span className="dim">[line {w.line}] </span>}
                  {w.key && <span>{w.key}: </span>}
                  {w.message}
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Import hard error */}
      {importError && (
        <div style={{ marginTop: 10, padding: '8px 12px', background: 'var(--err-dim)', borderLeft: '3px solid var(--err)', borderRadius: 'var(--r)', fontSize: 12 }}>
          <div style={{ fontWeight: 600, marginBottom: 4 }}>Import rejected</div>
          {importError.hard_errors.map((e, i) => (
            <div key={i} className="mono" style={{ fontSize: 12 }}>{e}</div>
          ))}
          {importError.warnings.length > 0 && (
            <>
              <div style={{ fontWeight: 600, marginTop: 8, marginBottom: 4 }}>Warnings (from partial parse)</div>
              {importError.warnings.map((w, i) => (
                <div key={i} className="mono" style={{ fontSize: 12 }}>
                  {w.line != null && <span className="dim">[line {w.line}] </span>}
                  {w.message}
                </div>
              ))}
            </>
          )}
        </div>
      )}

      {/* Export error */}
      {exportError && (
        <div style={{ marginTop: 10, padding: '8px 12px', background: 'var(--err-dim)', borderLeft: '3px solid var(--err)', borderRadius: 'var(--r)', fontSize: 12 }}>
          Export failed: {exportError}
        </div>
      )}
    </div>
  )
}
