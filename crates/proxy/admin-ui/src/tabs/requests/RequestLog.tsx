import { useSearchParams } from 'react-router-dom'
import { useRequests } from '../../api/queries'
import FeedRow from '../../components/feed/FeedRow'
import Pagination from '../../components/shared/Pagination'
import AsyncBoundary from '../../components/shared/AsyncBoundary'

export default function RequestLog() {
  const [params, setParams] = useSearchParams()
  const page = Math.max(1, Number(params.get('page') ?? '1') || 1)
  const backend = params.get('backend') ?? ''
  const status = params.get('status') ?? ''

  function updateParam(key: string, value: string, opts?: { resetPage?: boolean }) {
    const next = new URLSearchParams(params)
    if (value) next.set(key, value); else next.delete(key)
    if (opts?.resetPage) next.delete('page')
    setParams(next, { replace: true })
  }

  function setPage(updater: (p: number) => number) {
    const next = new URLSearchParams(params)
    const newPage = updater(page)
    if (newPage <= 1) next.delete('page'); else next.set('page', String(newPage))
    setParams(next, { replace: true })
  }

  const query = useRequests({ page, page_size: 50, backend, status })

  return (
    <div>
      <div className="section-header">
        <span className="section-label">Request Log</span>
        <div className="form-row" style={{ marginTop: 0 }}>
          <select
            name="requestlog-backend"
            value={backend}
            onChange={(e) => updateParam('backend', e.target.value, { resetPage: true })}
          >
            <option value="">All backends</option>
          </select>
          <select
            name="requestlog-status"
            value={status}
            onChange={(e) => updateParam('status', e.target.value, { resetPage: true })}
          >
            <option value="">All status</option>
            <option value="ok">2xx</option>
            <option value="error">4xx/5xx</option>
          </select>
        </div>
      </div>
      <AsyncBoundary
        query={query}
        errorTitle="Failed to load request log"
        empty={{
          when: (d) => d.requests.length === 0 && page === 1,
          render: () => (
            <div className="empty-cta">
              <div className="empty-cta-title">No requests logged</div>
              <div className="empty-cta-body">
                Send a request through the proxy and it will appear here. Only proxied traffic is logged;
                admin API calls are in the Audit tab.
              </div>
            </div>
          ),
        }}
      >
        {(data) => (
          <>
            <div className="feed">
              <div className="feed-header">
                <span>Time</span><span>Status</span><span>Latency</span>
                <span>Model</span><span>In</span><span>Out</span><span>Cost</span>
              </div>
              {data.requests.map((r) => <FeedRow key={r.request_id} req={r} />)}
            </div>
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
