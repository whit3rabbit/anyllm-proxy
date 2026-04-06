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

/**
 * Shared CSRF + response handling for all state-mutating requests.
 * Content-Type omitted when body is FormData so the browser can set the multipart boundary.
 * On 204 No Content, returns undefined.
 */
async function withCsrf<T>(
  method: 'POST' | 'PUT' | 'DELETE' | 'PATCH',
  path: string,
  body?: BodyInit,
  contentType?: string,
): Promise<T> {
  const csrfRes = await fetch('/admin/csrf-token', {
    headers: { 'Authorization': `Bearer ${getToken()}` },
  })
  if (!csrfRes.ok) throw new Error('Failed to fetch CSRF token')
  const { csrf_token: csrfToken } = await csrfRes.json() as { csrf_token: string }

  const res = await fetch(path, {
    method,
    headers: {
      'Authorization': `Bearer ${getToken()}`,
      'X-CSRF-Token': csrfToken,
      ...(contentType ? { 'Content-Type': contentType } : {}),
    },
    body,
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

/**
 * Send a state-mutating JSON request with CSRF protection.
 * On 204 No Content, returns undefined. Declare T as void for DELETE/no-body endpoints.
 */
export function mutatingFetch<T>(
  method: 'POST' | 'PUT' | 'DELETE' | 'PATCH',
  path: string,
  body?: unknown,
): Promise<T> {
  return withCsrf<T>(
    method,
    path,
    body !== undefined ? JSON.stringify(body) : undefined,
    body !== undefined ? 'application/json' : undefined,
  )
}

/**
 * Send a multipart/form-data POST with CSRF protection.
 * Content-Type is intentionally omitted so the browser sets the multipart boundary.
 */
export function mutatingFetchMultipart<T>(path: string, formData: FormData): Promise<T> {
  return withCsrf<T>('POST', path, formData)
}
