/** Supported HTTP request methods for database mutations. */
export type MutationMethod = 'POST' | 'PUT' | 'DELETE' | 'PATCH'

/** Minimal interface representing an HTTP response compatible with CSRF handlers. */
export interface CsrfResponseLike {
  /** True if response was successful (status 2xx). */
  ok: boolean
  /** HTTP response status code. */
  status: number
  /** Response headers interface. */
  headers: {
    get(name: string): string | null
  }
  /** Parses response body as JSON. */
  json(): Promise<unknown>
}

/** Function type mimicking the browser fetch API. */
export type FetchLike<TResponse extends CsrfResponseLike> = (
  input: string,
  init?: RequestInit,
) => Promise<TResponse>

/** Dependencies required to execute a CSRF mutation. */
export interface CsrfMutationDeps<TResponse extends CsrfResponseLike> {
  /** Fetch implementation to use. */
  fetchImpl: FetchLike<TResponse>
  /** Function retrieving the current authentication token. */
  getToken: () => string
  /** Error/auth response handler. */
  handleAuthAndErrors: (res: TResponse) => Promise<void>
}

/** A deferred task returning a promise. */
export type MutationTask<T> = () => Promise<T>

/**
 * Creates a queue to serialize mutating API requests, preventing concurrent race
 * conditions when refreshing and using CSRF tokens.
 */
export function createMutationQueue() {
  let tail: Promise<unknown> = Promise.resolve()

  return function queueMutation<T>(task: MutationTask<T>): Promise<T> {
    const run = tail.then(task, task)
    tail = run.catch(() => undefined)
    return run
  }
}

/** Global mutation queue instance. */
export const enqueueCsrfMutation = createMutationQueue()

async function fetchFreshCsrfToken<TResponse extends CsrfResponseLike>(
  fetchImpl: FetchLike<TResponse>,
  getToken: () => string,
): Promise<string> {
  const csrfRes = await fetchImpl('/admin/csrf-token', {
    headers: { 'Authorization': `Bearer ${getToken()}` },
  })
  if (!csrfRes.ok) throw new Error('Failed to fetch CSRF token')

  const payload = await csrfRes.json() as { csrf_token?: unknown }
  if (typeof payload.csrf_token !== 'string') {
    throw new Error('Failed to fetch CSRF token')
  }
  return payload.csrf_token
}

function mutationHeaders(token: string, authToken: string, contentType?: string): Record<string, string> {
  return {
    'Authorization': `Bearer ${authToken}`,
    'X-CSRF-Token': token,
    ...(contentType ? { 'Content-Type': contentType } : {}),
  }
}

function hasNoBody(res: CsrfResponseLike): boolean {
  return res.status === 204 || res.headers.get('content-length') === '0'
}

/**
 * Executes an HTTP mutation (POST/PUT/DELETE/PATCH) by first fetching a fresh CSRF token,
 * attaching the headers, and executing the request. Retries on 403.
 */
export async function runCsrfMutation<T, TResponse extends CsrfResponseLike>(
  method: MutationMethod,
  path: string,
  body: BodyInit | undefined,
  contentType: string | undefined,
  deps: CsrfMutationDeps<TResponse>,
): Promise<T> {
  const attempt = async (csrfToken: string): Promise<TResponse> => deps.fetchImpl(path, {
    method,
    headers: mutationHeaders(csrfToken, deps.getToken(), contentType),
    body,
  })

  let csrfToken = await fetchFreshCsrfToken(deps.fetchImpl, deps.getToken)
  let res = await attempt(csrfToken)

  if (res.status === 403) {
    csrfToken = await fetchFreshCsrfToken(deps.fetchImpl, deps.getToken)
    res = await attempt(csrfToken)
  }

  await deps.handleAuthAndErrors(res)
  if (hasNoBody(res)) return undefined as T
  return res.json() as Promise<T>
}

