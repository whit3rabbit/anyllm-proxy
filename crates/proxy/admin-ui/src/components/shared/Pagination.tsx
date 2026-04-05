interface PaginationProps {
  page: number
  hasMore: boolean
  onPrev: () => void
  onNext: () => void
}

export default function Pagination({ page, hasMore, onPrev, onNext }: PaginationProps) {
  return (
    <div className="pagination">
      <button className="btn btn-secondary btn-sm" onClick={onPrev} disabled={page <= 1}>
        Prev
      </button>
      <span>Page {page}</span>
      <button className="btn btn-secondary btn-sm" onClick={onNext} disabled={!hasMore}>
        Next
      </button>
    </div>
  )
}
