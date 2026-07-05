import { Fragment, useRef, useState } from 'react'
import {
  useConfig, useSaveConfig, useDeleteConfigOverride, useEnv,
  useImportEnv, downloadEnvExport,
} from '../../api/queries'
import EmptyState from '../../components/shared/EmptyState'
import ConfirmDialog from '../../components/shared/ConfirmDialog'
import { AdminButton, AdminSurface } from '../../components/shared/Performative'
import type { EnvImportResponse, EnvImportError } from '../../api/types'
import { ManagedBackendsSection } from './ManagedBackendsSection'

const RESTART_KEY = 'env_import_pending_restart'

function restartPending() {
  return sessionStorage.getItem(RESTART_KEY) === '1'
}

export default function Settings({ configured = true }: { configured?: boolean }) {
  const { data: cfg, isLoading, error } = useConfig()
  const { data: envData } = useEnv()
  const save = useSaveConfig()
  const del = useDeleteConfigOverride()
  const importEnv = useImportEnv()
  const fileRef = useRef<HTMLInputElement>(null)

  const [form, setForm] = useState<Record<string, string>>({})
  const [importResult, setImportResult] = useState<EnvImportResponse | null>(null)
  const [importError, setImportError] = useState<EnvImportError | null>(null)
  const [exportError, setExportError] = useState<string | null>(null)
  const [showRestartBanner, setShowRestartBanner] = useState(restartPending)
  const [pendingReset, setPendingReset] = useState<string | null>(null)

  function doReset() {
    if (!pendingReset) return Promise.resolve()
    const key = pendingReset
    return del.mutateAsync(key).then(() => undefined)
  }

  function handleSave(key: string, currentValue: string) {
    save.mutate({ [key]: form[key] ?? currentValue })
  }

  function handleBooleanSave(key: string, value: boolean) {
    save.mutate({ [key]: value })
  }

  // pxpipe model scope is a CSV of model bases; a model is "in scope" when any
  // base is a substring of its id (mirrors the backend's model_in_scope).
  function pxpipeScope(): string[] {
    return (cfg?.pxpipe_models ?? '').split(',').map((s) => s.trim()).filter(Boolean)
  }
  function pxpipeModelChecked(model: string): boolean {
    const m = model.toLowerCase()
    return pxpipeScope().some((base) => m.includes(base.toLowerCase()))
  }
  function togglePxpipeModel(model: string, on: boolean) {
    const cur = pxpipeScope()
    const next = on
      ? (pxpipeModelChecked(model) ? cur : [...cur, model])
      : cur.filter((base) => !model.toLowerCase().includes(base.toLowerCase()))
    save.mutate({ pxpipe_models: next.join(',') })
  }

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
    <div>
      {/* Managed backends — always shown first */}
      <ManagedBackendsSection />

      {/* Getting-started notice — shown when no backend is configured */}
      {!configured && (
        <div style={{ marginBottom: 20, padding: '12px 16px', border: '1px solid var(--border)', borderLeft: '3px solid var(--warn)', borderRadius: 'var(--r)', fontSize: 13 }}>
          <div style={{ fontWeight: 600, marginBottom: 8 }}>No proxy configuration found — nothing to forward requests to.</div>
          <div style={{ marginBottom: 10 }}>
            The proxy needs a backend endpoint (where to forward) and a listen port (where to accept).
            LISTEN_PORT defaults to 3000. Create a <span className="mono">.anyllm.env</span> and import it below,
            or pass it at startup: <span className="mono">anyllm-proxy --webui --env-file .anyllm.env</span>
          </div>
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 10 }}>
            <div>
              <div style={{ fontWeight: 600, marginBottom: 4, fontSize: 12 }}>OpenAI</div>
              <pre style={{ margin: 0, padding: '6px 10px', background: 'var(--surface-2)', borderRadius: 'var(--r)', fontSize: 11, overflowX: 'auto' }}>
{`OPENAI_API_KEY=sk-...
PROXY_API_KEYS=my-key`}
              </pre>
            </div>
            <div>
              <div style={{ fontWeight: 600, marginBottom: 4, fontSize: 12 }}>Ollama / local LLM</div>
              <pre style={{ margin: 0, padding: '6px 10px', background: 'var(--surface-2)', borderRadius: 'var(--r)', fontSize: 11, overflowX: 'auto' }}>
{`OPENAI_BASE_URL=http://localhost:11434/v1
PROXY_OPEN_RELAY=true`}
              </pre>
            </div>
            <div>
              <div style={{ fontWeight: 600, marginBottom: 4, fontSize: 12 }}>OpenRouter / custom</div>
              <pre style={{ margin: 0, padding: '6px 10px', background: 'var(--surface-2)', borderRadius: 'var(--r)', fontSize: 11, overflowX: 'auto' }}>
{`OPENAI_BASE_URL=https://openrouter.ai/api/v1
OPENAI_API_KEY=sk-or-...
PROXY_API_KEYS=my-key`}
              </pre>
            </div>
          </div>
        </div>
      )}

      {/* Restart-required banner — shown after a successful import */}
      {showRestartBanner && (
        <AdminSurface className="settings-restart-banner">
          <span>Restart the proxy for imported env vars to take effect.</span>
          <AdminButton size="sm" onClick={dismissRestartBanner}>Dismiss</AdminButton>
        </AdminSurface>
      )}

      {/* Env file import / export */}
      <div style={{ marginBottom: 24 }}>
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

      <EmptyState loading={isLoading} error={error?.message} />
      {cfg && (
        <div>
          <div className="section-label" style={{ marginBottom: 8 }}>Runtime</div>
          <div className="form-group">
            <label className="form-label" htmlFor="cfg-redact-secrets" style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <input
                id="cfg-redact-secrets"
                type="checkbox"
                checked={cfg.redact_secrets}
                disabled={save.isPending}
                onChange={(e) => handleBooleanSave('redact_secrets', e.target.checked)}
              />
              Redact secrets
            </label>
            {cfg.overridden_keys.includes('redact_secrets') && (
              <div className="form-row">
                <AdminButton size="sm" onClick={() => setPendingReset('redact_secrets')}>
                  Reset
                </AdminButton>
              </div>
            )}
          </div>

          <div className="form-group">
            <label className="form-label" htmlFor="cfg-log-bodies" style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <input
                id="cfg-log-bodies"
                type="checkbox"
                checked={cfg.log_bodies}
                disabled={save.isPending}
                onChange={(e) => handleBooleanSave('log_bodies', e.target.checked)}
              />
              Log bodies
            </label>
            {cfg.overridden_keys.includes('log_bodies') && (
              <div className="form-row">
                <AdminButton size="sm" onClick={() => setPendingReset('log_bodies')}>
                  Reset
                </AdminButton>
              </div>
            )}
          </div>

          <div className="form-group">
            <label className="form-label" htmlFor="cfg-thinking-repair" style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <input
                id="cfg-thinking-repair"
                type="checkbox"
                checked={cfg.anthropic_thinking_repair}
                disabled={save.isPending}
                onChange={(e) => handleBooleanSave('anthropic_thinking_repair', e.target.checked)}
              />
              Anthropic thinking-block repair
            </label>
            <div className="dim" style={{ fontSize: 12 }}>
              Repairs corrupted thinking/redacted_thinking blocks in Anthropic passthrough
              requests (applies to any backend running in BACKEND=anthropic passthrough mode,
              including a named backend in a multi-backend config). Off by default.
            </div>
            {cfg.overridden_keys.includes('anthropic_thinking_repair') && (
              <div className="form-row">
                <AdminButton size="sm" onClick={() => setPendingReset('anthropic_thinking_repair')}>
                  Reset
                </AdminButton>
              </div>
            )}
          </div>

          <div className="form-group">
            <label className="form-label" htmlFor="cfg-pxpipe-compress" style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <input
                id="cfg-pxpipe-compress"
                type="checkbox"
                checked={cfg.pxpipe_compress}
                disabled={save.isPending}
                onChange={(e) => handleBooleanSave('pxpipe_compress', e.target.checked)}
              />
              Image context compression (pxpipe)
            </label>
            <div className="dim" style={{ fontSize: 12 }}>
              Renders the stable system + tool-definition slab of Anthropic passthrough requests to a
              PNG image block to save input tokens on vision models. Off by default. Enable per-model
              below — only models that read imaged text reliably are offered.
            </div>
            {cfg.overridden_keys.includes('pxpipe_compress') && (
              <div className="form-row">
                <AdminButton size="sm" onClick={() => setPendingReset('pxpipe_compress')}>
                  Reset
                </AdminButton>
              </div>
            )}
            {cfg.pxpipe_compress && (
              <div style={{ marginTop: 8 }}>
                <div className="form-label" style={{ fontSize: 13 }}>Models in scope (vision-capable)</div>
                {cfg.pxpipe_available_models.length === 0 ? (
                  <div className="dim" style={{ fontSize: 12 }}>No vision-capable models in the catalog.</div>
                ) : (
                  <div style={{ display: 'flex', flexWrap: 'wrap', gap: '4px 16px' }}>
                    {cfg.pxpipe_available_models.map((model) => (
                      <label key={model} style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 12 }}>
                        <input
                          type="checkbox"
                          checked={pxpipeModelChecked(model)}
                          disabled={save.isPending}
                          onChange={(e) => togglePxpipeModel(model, e.target.checked)}
                        />
                        {model}
                      </label>
                    ))}
                  </div>
                )}
                {cfg.overridden_keys.includes('pxpipe_models') && (
                  <div className="form-row" style={{ marginTop: 6 }}>
                    <AdminButton size="sm" onClick={() => setPendingReset('pxpipe_models')}>
                      Reset scope
                    </AdminButton>
                  </div>
                )}
              </div>
            )}
          </div>

          <div className="form-group">
            <label className="form-label" htmlFor="cfg-forward-client-auth" style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <input
                id="cfg-forward-client-auth"
                type="checkbox"
                checked={cfg.forward_client_auth}
                disabled={save.isPending}
                onChange={(e) => handleBooleanSave('forward_client_auth', e.target.checked)}
              />
              Forward client credential (Anthropic passthrough)
            </label>
            <div className="dim" style={{ fontSize: 12 }}>
              Forwards the client's own x-api-key/Authorization header upstream instead of the
              operator's configured credential (BACKEND=anthropic passthrough only, single-key/BYOK
              deployments). The proxy refuses to enable this with 2+ PROXY_API_KEYS entries and no
              PROXY_OPEN_RELAY. Off by default.
            </div>
            {cfg.overridden_keys.includes('forward_client_auth') && (
              <div className="form-row">
                <AdminButton size="sm" onClick={() => setPendingReset('forward_client_auth')}>
                  Reset
                </AdminButton>
              </div>
            )}
          </div>

          <div className="form-group">
            <label className="form-label" htmlFor="cfg-tool-guardrail-mode">Tool guardrail mode</label>
            <div className="form-row">
              <select
                id="cfg-tool-guardrail-mode"
                value={cfg.tool_guardrail_mode}
                disabled={save.isPending}
                onChange={(e) => save.mutate({ tool_guardrail_mode: e.target.value })}
              >
                <option value="disabled">Disabled</option>
                <option value="standard">Standard</option>
              </select>
              {cfg.overridden_keys.includes('tool_guardrail_mode') && (
                <AdminButton size="sm" onClick={() => setPendingReset('tool_guardrail_mode')}>
                  Reset
                </AdminButton>
              )}
            </div>
            <div className="dim" style={{ fontSize: 12 }}>
              Applies advisory guardrails to tool calls the proxy auto-executes. Disabled by default.
            </div>
          </div>

          {cfg.entries.filter((entry) => !['redact_secrets', 'log_bodies', 'anthropic_thinking_repair', 'pxpipe_compress', 'pxpipe_models', 'forward_client_auth', 'tool_guardrail_mode'].includes(entry.key)).map((entry) => {
            const inputId = `cfg-${entry.key}`
            return (
              <div className="form-group" key={entry.key}>
                <label className="form-label" htmlFor={inputId}>{entry.key}</label>
                <div className="form-row">
                  <input
                    id={inputId}
                    name={entry.key}
                    value={form[entry.key] ?? entry.value}
                    onChange={(e) => setForm((f) => ({ ...f, [entry.key]: e.target.value }))}
                  />
                  <AdminButton tone="primary" size="sm" onClick={() => handleSave(entry.key, entry.value)}>Save</AdminButton>
                  <AdminButton size="sm" onClick={() => setPendingReset(entry.key)}>Reset</AdminButton>
                </div>
              </div>
            )
          })}
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

      <ConfirmDialog
        open={pendingReset !== null}
        onClose={() => setPendingReset(null)}
        onConfirm={doReset}
        title="Reset override?"
        message={
          <>
            Reset override for <span className="mono">{pendingReset}</span>? The runtime value will revert
            to the env-file or default. Active connections are not affected.
          </>
        }
        confirmLabel="Reset"
        variant="primary"
      />
    </div>
  )
}
