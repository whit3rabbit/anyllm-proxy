import { useAuthStore } from '../store/auth'

function getToken(): string {
  return useAuthStore.getState().token ?? ''
}

export async function apiFetch<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    ...options,
    headers: {
      'Authorization': `Bearer ${getToken()}`,
      ...(options?.headers ?? {}),
    },
  })
  if (res.status === 401) {
    useAuthStore.getState().logout()
    throw new Error('Unauthorized')
  }
  if (!res.ok) {
    const text = await res.text().catch(() => res.statusText)
    throw new Error(text || `HTTP ${res.status}`)
  }
  return res.json() as Promise<T>
}

export async function mutatingFetch<T>(
  method: 'POST' | 'PUT' | 'DELETE' | 'PATCH',
  path: string,
  body?: unknown,
): Promise<T> {
  // Fetch a one-time CSRF token before every state-mutating request.
  const csrfRes = await fetch('/admin/csrf-token', {
    headers: { 'Authorization': `Bearer ${getToken()}` },
  })
  if (!csrfRes.ok) throw new Error('Failed to fetch CSRF token')
  const { token: csrfToken } = await csrfRes.json() as { token: string }

  const res = await fetch(path, {
    method,
    headers: {
      'Authorization': `Bearer ${getToken()}`,
      'X-CSRF-Token': csrfToken,
      ...(body !== undefined ? { 'Content-Type': 'application/json' } : {}),
    },
    body: body !== undefined ? JSON.stringify(body) : undefined,
  })
  if (res.status === 401) {
    useAuthStore.getState().logout()
    throw new Error('Unauthorized')
  }
  if (!res.ok) {
    const text = await res.text().catch(() => res.statusText)
    throw new Error(text || `HTTP ${res.status}`)
  }
  if (res.status === 204 || res.headers.get('content-length') === '0') {
    return undefined as T
  }
  return res.json() as Promise<T>
}
