import { useState } from 'react'
import {
  useSaveConfig, useDeleteConfigOverride,
  useOptimizerModel, useDownloadOptimizerModel,
} from '../../api/queries'
import ConfirmDialog from '../../components/shared/ConfirmDialog'
import { AdminButton } from '../../components/shared/Performative'
import type { ConfigResponse } from '../../api/types'

function fmtMB(bytes: number): string {
  return `${Math.round(bytes / 1_000_000)} MB`
}

interface RuntimeSettingsSectionProps {
  cfg: ConfigResponse
}

export default function RuntimeSettingsSection({ cfg }: RuntimeSettingsSectionProps) {
  const save = useSaveConfig()
  const del = useDeleteConfigOverride()
  const { data: model } = useOptimizerModel()
  const downloadModel = useDownloadOptimizerModel()

  const [form, setForm] = useState<Record<string, string>>({})
  const [pendingReset, setPendingReset] = useState<string | null>(null)

  /** Resets a config override key back to its default value. */
  function doReset() {
    if (!pendingReset) return Promise.resolve()
    const key = pendingReset
    return del.mutateAsync(key).then(() => undefined)
  }

  /** Saves a text configuration setting value. */
  function handleSave(key: string, currentValue: string) {
    save.mutate({ [key]: form[key] ?? currentValue })
  }

  /** Saves a boolean configuration setting value. */
  function handleBooleanSave(key: string, value: boolean) {
    save.mutate({ [key]: value })
  }

  // pxpipe model scope is a CSV of model bases; a model is "in scope" when any
  // base is a substring of its id (mirrors the backend's model_in_scope).
  function pxpipeScope(): string[] {
    return (cfg.pxpipe_models ?? '').split(',').map((s) => s.trim()).filter(Boolean)
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

  return (
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
        <label className="form-label" htmlFor="cfg-rtk-compress" style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <input
            id="cfg-rtk-compress"
            type="checkbox"
            checked={cfg.rtk_compress}
            disabled={save.isPending}
            onChange={(e) => handleBooleanSave('rtk_compress', e.target.checked)}
          />
          Tool-output compression (RTK)
        </label>
        <div className="dim" style={{ fontSize: 12 }}>
          Command-aware filtering of tool-result text (test/build/git/log output) using the RTK
          filter catalog. Shrinks noisy machine output before it reaches the backend; deterministic
          and cache-safe. Off by default. Applies to Anthropic passthrough and translate paths.
        </div>
        {cfg.overridden_keys.includes('rtk_compress') && (
          <div className="form-row">
            <AdminButton size="sm" onClick={() => setPendingReset('rtk_compress')}>
              Reset
            </AdminButton>
          </div>
        )}
        {cfg.rtk_compress && (
          <div style={{ marginTop: 8 }}>
            <div className="form-label" style={{ fontSize: 13 }}>Models in scope (CSV, empty = all)</div>
            <input
              type="text"
              className="form-input"
              key={cfg.rtk_models}
              defaultValue={cfg.rtk_models}
              placeholder="empty = all models; e.g. claude, gpt-5"
              disabled={save.isPending}
              onBlur={(e) => {
                const v = e.target.value.trim()
                if (v === (cfg.rtk_models ?? '')) return
                save.mutate({ rtk_models: v })
              }}
            />
            {cfg.overridden_keys.includes('rtk_models') && (
              <div className="form-row" style={{ marginTop: 6 }}>
                <AdminButton size="sm" onClick={() => setPendingReset('rtk_models')}>
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

      <div className="form-group">
        <label className="form-label" htmlFor="cfg-optimizer-mode">Prompt compression (optimizer)</label>
        <div className="form-row">
          <select
            id="cfg-optimizer-mode"
            value={cfg.optimizer_mode}
            disabled={save.isPending || (!!model?.compiled_in && !model?.present)}
            onChange={(e) => save.mutate({ optimizer_mode: e.target.value })}
          >
            <option value="off">Off</option>
            <option value="shadow">Shadow (report only)</option>
            <option value="live">Live (compress)</option>
          </select>
          {cfg.overridden_keys.includes('optimizer_mode') && (
            <AdminButton size="sm" onClick={() => setPendingReset('optimizer_mode')}>
              Reset
            </AdminButton>
          )}
        </div>
        <div className="dim" style={{ fontSize: 12 }}>
          Frozen-Frontier compression of long conversation history (latest turn untouched). Off by default.
        </div>

        {model && !model.compiled_in && (
          <div className="dim" style={{ fontSize: 12, marginTop: 8 }}>
            Heuristic scorer only. Rebuild the proxy with <code>--features optimizer-onnx</code> to enable the LLMLingua-2 ONNX scorer.
          </div>
        )}
        {model?.compiled_in && !model.present && !model.downloading && (
          <div className="form-row" style={{ marginTop: 8 }}>
            <AdminButton
              size="sm"
              disabled={downloadModel.isPending}
              onClick={() => downloadModel.mutate()}
            >
              Download model ({fmtMB(model.size_bytes)})
            </AdminButton>
            <span className="dim" style={{ fontSize: 12 }}>
              Required before enabling. Verified against a pinned sha256.
            </span>
          </div>
        )}
        {model?.downloading && (
          <div className="dim" style={{ fontSize: 12, marginTop: 8 }}>
            Downloading and verifying model ({fmtMB(model.size_bytes)})…
          </div>
        )}
        {model?.error && !model.downloading && (
          <div style={{ fontSize: 12, marginTop: 8, color: 'var(--danger, #c0392b)' }}>
            Download failed: {model.error}
          </div>
        )}
        {model?.compiled_in && model.present && (
          <div className="dim" style={{ fontSize: 12, marginTop: 8 }}>
            ONNX scorer ready — live mode uses LLMLingua-2 (loaded on the next request).
          </div>
        )}
      </div>

      {cfg.entries.filter((entry) => !['redact_secrets', 'log_bodies', 'anthropic_thinking_repair', 'pxpipe_compress', 'pxpipe_models', 'rtk_compress', 'rtk_models', 'forward_client_auth', 'tool_guardrail_mode', 'optimizer_mode'].includes(entry.key)).map((entry) => {
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
