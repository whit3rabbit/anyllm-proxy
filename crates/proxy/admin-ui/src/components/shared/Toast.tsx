import { useEffect } from 'react'
import { useToastStore, type Toast } from '../../store/toast'

export default function ToastProvider() {
  const toasts = useToastStore((s) => s.toasts)
  if (toasts.length === 0) return null
  return (
    <div className="toast-stack" role="region" aria-label="Notifications">
      {toasts.map((t) => (
        <ToastItem key={t.id} toast={t} />
      ))}
    </div>
  )
}

function ToastItem({ toast }: { toast: Toast }) {
  const dismiss = useToastStore((s) => s.dismiss)

  useEffect(() => {
    if (toast.ttlMs == null) return
    const handle = window.setTimeout(() => dismiss(toast.id), toast.ttlMs)
    return () => window.clearTimeout(handle)
  }, [toast.id, toast.ttlMs, dismiss])

  return (
    <div className={`toast toast-${toast.variant}`} role="status">
      <div className="toast-message">{toast.message}</div>
      <button
        type="button"
        className="toast-close"
        aria-label="Dismiss"
        onClick={() => dismiss(toast.id)}
      >
        ×
      </button>
    </div>
  )
}
