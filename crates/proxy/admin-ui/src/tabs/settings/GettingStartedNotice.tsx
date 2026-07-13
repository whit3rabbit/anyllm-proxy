interface GettingStartedNoticeProps {
  configured: boolean
}

export default function GettingStartedNotice({ configured }: GettingStartedNoticeProps) {
  if (configured) return null

  return (
    <div style={{ marginBottom: 20, padding: '12px 16px', border: '1px solid var(--border)', borderLeft: '3px solid var(--warn)', borderRadius: 'var(--r)', fontSize: 13 }}>
      <div style={{ fontWeight: 600, marginBottom: 8 }}>No backend configured — nothing to forward requests to.</div>
      <div style={{ marginBottom: 10 }}>
        Add a backend on the <span className="mono">Backends</span> tab, or configure one via env.
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
  )
}
