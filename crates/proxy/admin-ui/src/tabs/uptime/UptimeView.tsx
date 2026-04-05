import { useUptime } from '../../api/queries'
import ProxyHealth from './ProxyHealth'
import BackendHealthRow from './BackendHealthRow'
import EmptyState from '../../components/shared/EmptyState'

export default function UptimeView() {
  const { data, isLoading, error } = useUptime()

  return (
    <div>
      <EmptyState loading={isLoading} error={error?.message} />
      {data && (
        <>
          <ProxyHealth proxy={data.proxy} />
          <div className="section-label" style={{ marginTop: 16, marginBottom: 8 }}>Backend Availability</div>
          <table className="backend-health-table">
            <thead>
              <tr>
                <th>Backend</th>
                <th>Status</th>
                <th>Uptime (30d)</th>
                <th>Last checked</th>
                <th>Latency</th>
                <th>History</th>
              </tr>
            </thead>
            <tbody>
              {data.backends
                .slice()
                .sort((a, b) => a.name.localeCompare(b.name))
                .map((b) => <BackendHealthRow key={b.name} b={b} />)}
            </tbody>
          </table>
        </>
      )}
    </div>
  )
}
