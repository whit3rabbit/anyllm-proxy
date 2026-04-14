import { create } from 'zustand'

export type ToastVariant = 'success' | 'error' | 'warn' | 'info'

export interface Toast {
  id: number
  variant: ToastVariant
  message: string
  ttlMs: number | null
}

interface ToastState {
  toasts: Toast[]
  push: (input: { variant: ToastVariant; message: string; ttlMs?: number | null }) => number
  dismiss: (id: number) => void
  clear: () => void
}

const MAX_TOASTS = 5
const DEFAULT_TTL_MS = 4_000

let nextId = 1

export const useToastStore = create<ToastState>((set) => ({
  toasts: [],
  push({ variant, message, ttlMs }) {
    const id = nextId++
    // Errors stay until dismissed so the user can read the retry hint.
    const resolvedTtl =
      ttlMs === undefined ? (variant === 'error' ? null : DEFAULT_TTL_MS) : ttlMs
    set((state) => {
      const next = [...state.toasts, { id, variant, message, ttlMs: resolvedTtl }]
      return { toasts: next.length > MAX_TOASTS ? next.slice(-MAX_TOASTS) : next }
    })
    return id
  },
  dismiss(id) {
    set((state) => ({ toasts: state.toasts.filter((t) => t.id !== id) }))
  },
  clear() {
    set({ toasts: [] })
  },
}))

/** Non-hook helper for modules that can't use hooks (e.g. api/client.ts). */
export function pushToast(
  input: { variant: ToastVariant; message: string; ttlMs?: number | null },
): number {
  return useToastStore.getState().push(input)
}
