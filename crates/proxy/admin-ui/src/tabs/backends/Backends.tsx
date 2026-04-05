import { useBackends } from '../../api/queries'
import EmptyState from '../../components/shared/EmptyState'
import StatusDot from '../../components/shared/StatusDot'

export default function Backends() {
  const { data, isLoading, error } = useBackends()

  return (
    <div>
      <EmptyState loading={isLoading} error={error?.message} empty={data?.length === 0} />
      <div className="backend-cards">
        {data?.map((b) => (
          <div className="card" key={b.name}>
            <div className="card-header">
              <span className="card-name">{b.name}</span>
              <StatusDot status={b.status === 'ok' ? 'ok' : 'err'} pulse={b.status === 'ok'} />
            </div>
            <div className="card-body">
              <div className="mono">{b.model}</div>
              <div style={{ marginTop: 6, display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 4 }}>
                <span className="dim">Requests</span><span className="mono">{b.requests_total}</span>
                <span className="dim">P50</span><span className="mono">{b.p50_ms}ms</span>
                <span className="dim">P95</span><span className="mono">{b.p95_ms}ms</span>
                <span className="dim">Errors</span>
                <span className="mono" style={{ color: b.requests_err > 0 ? 'var(--err)' : undefined }}>
                  {b.requests_err}
                </span>
              </div>
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
