import type { RequestLogEntry } from '../../api/types'

export default function FeedDetail({ req }: { req: RequestLogEntry }) {
  return (
    <div className="feed-detail">
      <span className="label">Request ID</span>
      <span className="val">{req.request_id}</span>
      <span className="label">Backend</span>
      <span className="val">{req.backend}</span>
      <span className="label">Model (req)</span>
      <span className="val">{req.model_requested ?? '—'}</span>
      <span className="label">Model (mapped)</span>
      <span className="val">{req.model_mapped ?? '—'}</span>
      <span className="label">Latency</span>
      <span className="val">{req.latency_ms} ms</span>
      <span className="label">Tokens in/out</span>
      <span className="val">
        {req.input_tokens ?? '—'} / {req.output_tokens ?? '—'}
      </span>
      <span className="label">Cost</span>
      <span className="val">
        {req.cost_usd != null ? `$${req.cost_usd.toFixed(6)}` : '—'}
      </span>
      {req.error_message && (
        <div className="error-msg">{req.error_message}</div>
      )}
    </div>
  )
}
