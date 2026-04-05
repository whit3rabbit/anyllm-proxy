import { useEffect, useRef, useState } from 'react'
import { useWsStore } from '../../store/ws'
import type { RequestLogEntry } from '../../api/types'
import FeedRow from './FeedRow'

const MAX_FEED = 200

export default function LiveFeed({ initial }: { initial?: RequestLogEntry[] }) {
  const [rows, setRows] = useState<RequestLogEntry[]>(initial ?? [])
  const [paused, setPaused] = useState(false)
  const pausedRef = useRef(paused)
  pausedRef.current = paused

  const lastEvent = useWsStore((s) => s.lastEvent)

  useEffect(() => {
    if (!lastEvent || lastEvent.type !== 'request_completed' || pausedRef.current) return
    setRows((prev) => [lastEvent.data, ...prev].slice(0, MAX_FEED))
  }, [lastEvent])

  return (
    <div>
      <div className="section-header">
        <span className="section-label">Live Feed</span>
        <button
          className={`btn btn-sm ${paused ? 'btn-primary' : 'btn-secondary'}`}
          onClick={() => setPaused((v) => !v)}
        >
          {paused ? 'Resume' : 'Pause'}
        </button>
      </div>
      <div className="feed">
        <div className="feed-header">
          <span>Time</span>
          <span>Status</span>
          <span>Latency</span>
          <span>Model</span>
          <span>In</span>
          <span>Out</span>
          <span>Cost</span>
        </div>
        {rows.length === 0 ? (
          <div className="empty">Waiting for requests…</div>
        ) : (
          rows.map((r) => <FeedRow key={r.request_id} req={r} />)
        )}
      </div>
    </div>
  )
}
