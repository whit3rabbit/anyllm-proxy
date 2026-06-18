import { useMemo } from 'react'
import { useBackends, useUptime } from '../../api/queries'
import type { BackendUptimeInfo } from '../../api/types'
import EmptyState from '../../components/shared/EmptyState'
import StatusDot from '../../components/shared/StatusDot'
import { AdminSurface } from '../../components/shared/Performative'

export default function Backends() {
  const { data, isLoading, error } = useBackends()
  const { data: uptime } = useUptime()

  // Real per-backend health/latency lives in the uptime endpoint (health_checks table),
  // keyed by backend name. get_backends only carries request counts.
  const healthMap = useMemo(() => {
    const m = new Map<string, BackendUptimeInfo>()
    for (const b of uptime?.backends ?? []) m.set(b.name, b)
    return m
  }, [uptime])

  return (
    <div>
      <EmptyState loading={isLoading} error={error?.message} empty={data?.length === 0} />
      <div className="backend-cards">
        {data?.map((b) => {
          const health = healthMap.get(b.name)
          const dot = health?.status === 'up' ? 'ok' : health?.status === 'down' ? 'err' : 'dim'
          return (
            <AdminSurface className="card" key={b.name} glowOnHover>
              <div className="card-header">
                <span className="card-name">{b.name}</span>
                <StatusDot status={dot} pulse={dot === 'ok'} />
              </div>
              <div className="card-body">
                <div className="mono">{b.big_model} / {b.small_model}</div>
                <div style={{ marginTop: 6, display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 4 }}>
                  <span className="dim">Requests</span><span className="mono">{b.metrics.requests_total}</span>
                  <span className="dim">Errors</span>
                  <span className="mono" style={{ color: b.metrics.requests_error > 0 ? 'var(--err)' : undefined }}>
                    {b.metrics.requests_error}
                  </span>
                  {health?.last_latency_ms != null && (
                    <>
                      <span className="dim">Last latency</span><span className="mono">{health.last_latency_ms}ms</span>
                    </>
                  )}
                  {health && (
                    <>
                      <span className="dim">30d uptime</span><span className="mono">{health.uptime_pct_30d}%</span>
                    </>
                  )}
                </div>
              </div>
            </AdminSurface>
          )
        })}
      </div>
    </div>
  )
}
