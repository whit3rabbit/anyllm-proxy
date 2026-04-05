import type { RouteMetrics } from '../../api/types'

export default function RouteTable({ routes }: { routes: RouteMetrics[] }) {
  const sorted = [...routes].sort((a, b) => b.requests_per_min - a.requests_per_min)
  return (
    <table className="route-table">
      <thead>
        <tr>
          <th>Route</th>
          <th>Req/min</th>
          <th>Error rate</th>
          <th>Avg payload</th>
          <th>P95 latency</th>
          <th>Total</th>
        </tr>
      </thead>
      <tbody>
        {sorted.map((r) => (
          <tr key={r.path}>
            <td className="mono">{r.path}</td>
            <td className="mono">{r.requests_per_min.toFixed(2)}</td>
            <td className="mono" style={{ color: r.error_rate > 0.05 ? 'var(--err)' : r.error_rate > 0.01 ? 'var(--warn)' : undefined }}>
              {(r.error_rate * 100).toFixed(1)}%
            </td>
            <td className="mono">{formatBytes(r.avg_request_bytes)}</td>
            <td className="mono">{r.p95_latency_ms}ms</td>
            <td className="mono">{r.total_requests.toLocaleString()}</td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}

function formatBytes(n: number) {
  if (n < 1024) return `${n}B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)}KB`
  return `${(n / (1024 * 1024)).toFixed(1)}MB`
}
