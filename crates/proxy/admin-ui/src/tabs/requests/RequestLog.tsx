import { useState } from 'react'
import { useRequests } from '../../api/queries'
import FeedRow from '../../components/feed/FeedRow'
import Pagination from '../../components/shared/Pagination'
import EmptyState from '../../components/shared/EmptyState'

export default function RequestLog() {
  const [page, setPage] = useState(1)
  const [backend, setBackend] = useState('')
  const [status, setStatus] = useState('')
  const { data, isLoading, error } = useRequests({ page, page_size: 50, backend, status })

  return (
    <div>
      <div className="section-header">
        <span className="section-label">Request Log</span>
        <div className="form-row" style={{ marginTop: 0 }}>
          <select value={backend} onChange={(e) => { setBackend(e.target.value); setPage(1) }}>
            <option value="">All backends</option>
          </select>
          <select value={status} onChange={(e) => { setStatus(e.target.value); setPage(1) }}>
            <option value="">All status</option>
            <option value="ok">2xx</option>
            <option value="error">4xx/5xx</option>
          </select>
        </div>
      </div>
      <EmptyState loading={isLoading} error={error?.message} />
      {data && (
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
    </div>
  )
}
