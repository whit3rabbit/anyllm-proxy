export type MutationMethod = 'POST' | 'PUT' | 'DELETE' | 'PATCH'

interface CsrfResponseLike {
  ok: boolean
  status: number
  headers: {
    get(name: string): string | null
  }
  json(): Promise<unknown>
}

type FetchLike<TResponse extends CsrfResponseLike> = (
  input: string,
  init?: RequestInit,
) => Promise<TResponse>

interface CsrfMutationDeps<TResponse extends CsrfResponseLike> {
  fetchImpl: FetchLike<TResponse>
  getToken: () => string
  handleAuthAndErrors: (res: TResponse) => Promise<void>
}

type MutationTask<T> = () => Promise<T>

export function createMutationQueue() {
  let tail: Promise<unknown> = Promise.resolve()

  return function queueMutation<T>(task: MutationTask<T>): Promise<T> {
    const run = tail.then(task, task)
    tail = run.catch(() => undefined)
    return run
  }
}

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
