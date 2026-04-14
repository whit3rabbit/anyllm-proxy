import { useSearchParams } from 'react-router-dom'
import { useAudit } from '../../api/queries'
import Pagination from '../../components/shared/Pagination'
import AsyncBoundary from '../../components/shared/AsyncBoundary'

export default function Audit() {
  const [params, setParams] = useSearchParams()
  const page = Math.max(1, Number(params.get('page') ?? '1') || 1)
  function setPage(updater: (p: number) => number) {
    const next = new URLSearchParams(params)
    const newPage = updater(page)
    if (newPage <= 1) next.delete('page'); else next.set('page', String(newPage))
    setParams(next, { replace: true })
  }
  const query = useAudit({ page, page_size: 50 })

  return (
    <div>
      <AsyncBoundary
        query={query}
        errorTitle="Failed to load audit log"
        empty={{
          when: (d) => d.entries.length === 0 && page === 1,
          render: () => (
            <div className="empty-cta">
              <div className="empty-cta-title">No audit entries yet</div>
              <div className="empty-cta-body">
                Admin actions (creating keys, editing routes, managing backends) are recorded here.
              </div>
            </div>
          ),
        }}
      >
        {(data) => (
          <>
            <table className="route-table">
              <thead>
                <tr><th>Time</th><th>Action</th><th>Target</th><th>Detail</th><th>IP</th></tr>
              </thead>
              <tbody>
                {data.entries.map((e) => (
                  <tr key={e.id}>
                    <td className="mono dim">{e.timestamp.slice(0, 19)}</td>
                    <td className="mono">{e.action}</td>
                    <td className="dim">{e.target_type}{e.target_id ? ` #${e.target_id}` : ''}</td>
                    <td className="dim audit-detail">{e.detail ?? '—'}</td>
                    <td className="mono dim">{e.source_ip ?? '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <Pagination
              page={page}
              hasMore={data.has_more}
              onPrev={() => setPage((p) => Math.max(1, p - 1))}
              onNext={() => setPage((p) => p + 1)}
            />
          </>
        )}
      </AsyncBoundary>
    </div>
  )
}
