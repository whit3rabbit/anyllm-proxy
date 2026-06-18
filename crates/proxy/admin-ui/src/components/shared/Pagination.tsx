import { AdminButton } from './Performative'

interface PaginationProps {
  page: number
  hasMore: boolean
  onPrev: () => void
  onNext: () => void
}

export default function Pagination({ page, hasMore, onPrev, onNext }: PaginationProps) {
  return (
    <div className="pagination">
      <AdminButton size="sm" onClick={onPrev} disabled={page <= 1}>
        Prev
      </AdminButton>
      <span>Page {page}</span>
      <AdminButton size="sm" onClick={onNext} disabled={!hasMore}>
        Next
      </AdminButton>
    </div>
  )
}
