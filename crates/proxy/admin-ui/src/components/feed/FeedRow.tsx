import { useState } from 'react'
import type { RequestLogEntry } from '../../api/types'
import FeedDetail from './FeedDetail'

function statusClass(code: number) {
  if (code < 300) return 'status-2xx'
  if (code < 500) return 'status-4xx'
  return 'status-5xx'
}

export default function FeedRow({ req }: { req: RequestLogEntry }) {
  const [open, setOpen] = useState(false)
  return (
    <>
      <div className="feed-row" onClick={() => setOpen((v) => !v)}>
        <span className="mono dim">{req.timestamp.slice(11, 19)}</span>
        <span className={`mono ${statusClass(req.status_code)}`}>{req.status_code}</span>
        <span className="mono">{req.latency_ms}ms</span>
        <span className="mono" style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
          {req.model_requested ?? req.backend}
          {req.is_streaming && <span className="streaming-badge">stream</span>}
        </span>
        <span className="mono dim">{req.input_tokens ?? '—'}</span>
        <span className="mono dim">{req.output_tokens ?? '—'}</span>
        <span className="mono dim">
          {req.cost_usd != null ? `$${req.cost_usd.toFixed(5)}` : '—'}
        </span>
      </div>
      {open && <FeedDetail req={req} />}
    </>
  )
}
