import { useAuthStore } from '../store/auth'
import { pushToast } from '../store/toast'
import { enqueueCsrfMutation, runCsrfMutation, type MutationMethod } from './csrf'

function getToken(): string {
  return useAuthStore.getState().token ?? ''
}

/** Thrown when the admin API responds 429. Lets callers present a toast and
 *  blocks React Query's default retry loop (we already retried once below). */
export class RateLimitError extends Error {
  readonly retryAfterSeconds: number
  constructor(retryAfterSeconds: number) {
    super(`Rate limited. Retry in ${retryAfterSeconds}s.`)
    this.name = 'RateLimitError'
    this.retryAfterSeconds = retryAfterSeconds
  }
}

function parseRetryAfter(res: Response): number {
  const raw = res.headers.get('retry-after')
  if (!raw) return 1
  const seconds = Number(raw)
  if (Number.isFinite(seconds) && seconds > 0) return Math.ceil(seconds)
  // HTTP-date form: compute delta. Fall back to 1s on parse failure.
  const asDate = Date.parse(raw)
  if (!Number.isNaN(asDate)) {
    return Math.max(1, Math.ceil((asDate - Date.now()) / 1000))
  }
  return 1
}

async function handleAuthAndErrors(res: Response): Promise<void> {
  if (res.status === 401) {
    useAuthStore.getState().logout()
    throw new Error('Unauthorized')
  }
  if (res.status === 429) {
    const retryAfter = parseRetryAfter(res)
    pushToast({
      variant: 'warn',
      message: `Admin API rate-limited. Retry in ${retryAfter}s.`,
      ttlMs: retryAfter * 1000,
    })
    throw new RateLimitError(retryAfter)
  }
  if (!res.ok) {
    const text = await res.text().catch(() => res.statusText)
    throw new Error(text || `HTTP ${res.status}`)
  }
}

export async function apiFetch<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    ...options,
    headers: {
      'Authorization': `Bearer ${getToken()}`,
      ...(options?.headers ?? {}),
    },
  })
  await handleAuthAndErrors(res)
  return res.json() as Promise<T>
}

/**
 * Shared CSRF + response handling for all state-mutating requests.
 * Content-Type omitted when body is FormData so the browser can set the multipart boundary.
 * On 204 No Content, returns undefined.
 */
async function withCsrf<T>(
  method: MutationMethod,
  path: string,
  body?: BodyInit,
  contentType?: string,
): Promise<T> {
  return enqueueCsrfMutation(() => runCsrfMutation<T, Response>(
    method,
    path,
    body,
    contentType,
    {
      fetchImpl: (input, init) => fetch(input, init),
      getToken,
      handleAuthAndErrors,
    },
  ))
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
