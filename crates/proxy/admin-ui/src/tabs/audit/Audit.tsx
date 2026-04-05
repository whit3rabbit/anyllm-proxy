import { useState } from 'react'
import { useAudit } from '../../api/queries'
import Pagination from '../../components/shared/Pagination'
import EmptyState from '../../components/shared/EmptyState'

export default function Audit() {
  const [page, setPage] = useState(1)
  const { data, isLoading, error } = useAudit({ page, page_size: 50 })

  return (
    <div>
      <EmptyState loading={isLoading} error={error?.message} />
      {data && (
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
                  <td className="dim" style={{ maxWidth: 300, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{e.detail ?? '—'}</td>
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
    </div>
  )
}
