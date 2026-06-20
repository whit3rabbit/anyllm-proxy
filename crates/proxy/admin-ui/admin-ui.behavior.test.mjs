import assert from 'node:assert/strict'
import { mkdtemp, readFile, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { basename, join } from 'node:path'
import { pathToFileURL } from 'node:url'
import test from 'node:test'
import { transformWithOxc } from 'vite'

async function importTs(sourcePath) {
  const source = await readFile(new URL(sourcePath, import.meta.url), 'utf8')
  const result = await transformWithOxc(source, sourcePath, { lang: 'ts' })
  const dir = await mkdtemp(join(tmpdir(), 'anyllm-admin-ui-test-'))
  const outputPath = join(dir, basename(sourcePath).replace(/\.ts$/, '.mjs'))
  await writeFile(outputPath, result.code)
  return import(pathToFileURL(outputPath).href)
}

function jsonResponse(body, init = {}) {
  return {
    ok: init.ok ?? true,
    status: init.status ?? 200,
    headers: {
      get(name) {
        return name.toLowerCase() === 'content-length' ? (init.contentLength ?? null) : null
      },
    },
    async json() {
      return body
    },
    async text() {
      return typeof body === 'string' ? body : JSON.stringify(body)
    },
  }
}

async function waitFor(predicate) {
  for (let i = 0; i < 20; i += 1) {
    if (predicate()) return
    await new Promise((resolve) => setTimeout(resolve, 0))
  }
  assert.ok(predicate(), 'condition was not met')
}

test('key creation payload uses enforced max_budget_usd instead of legacy spend_limit', async () => {
  const { buildCreateKeyPayload } = await importTs('./src/tabs/keys/keyPayload.ts')

  const payload = buildCreateKeyPayload({
    description: '',
    spendLimit: '12.50',
    rpmLimit: '',
  })

  assert.deepEqual(payload, {
    description: null,
    max_budget_usd: 12.5,
    rpm_limit: null,
  })
  assert.equal('spend_limit' in payload, false)
})

test('csrf mutations run FIFO and fetch a fresh token inside each queued mutation', async () => {
  const { createMutationQueue, runCsrfMutation } = await importTs('./src/api/csrf.ts')
  const queueMutation = createMutationQueue()
  const calls = []
  const releaseMutations = []
  let csrfCount = 0

  const fetchImpl = async (path, init = {}) => {
    const headers = init.headers ?? {}
    if (path === '/admin/csrf-token') {
      csrfCount += 1
      calls.push({ kind: 'csrf', token: `csrf-${csrfCount}` })
      return jsonResponse({ csrf_token: `csrf-${csrfCount}` })
    }
    calls.push({ kind: 'mutation', path, token: headers['X-CSRF-Token'] })
    await new Promise((resolve) => releaseMutations.push(resolve))
    return jsonResponse({ path })
  }

  const deps = {
    fetchImpl,
    getToken: () => 'admin-token',
    handleAuthAndErrors: async () => {},
  }
  const first = queueMutation(() => runCsrfMutation('POST', '/admin/api/one', undefined, 'application/json', deps))
  const second = queueMutation(() => runCsrfMutation('POST', '/admin/api/two', undefined, 'application/json', deps))

  await waitFor(() => calls.length === 2)
  assert.deepEqual(calls, [
    { kind: 'csrf', token: 'csrf-1' },
    { kind: 'mutation', path: '/admin/api/one', token: 'csrf-1' },
  ])

  releaseMutations.shift()()
  assert.deepEqual(await first, { path: '/admin/api/one' })
  await waitFor(() => calls.length === 4)
  assert.deepEqual(calls, [
    { kind: 'csrf', token: 'csrf-1' },
    { kind: 'mutation', path: '/admin/api/one', token: 'csrf-1' },
    { kind: 'csrf', token: 'csrf-2' },
    { kind: 'mutation', path: '/admin/api/two', token: 'csrf-2' },
  ])

  releaseMutations.shift()()
  assert.deepEqual(await second, { path: '/admin/api/two' })
})

test('csrf mutation retries one 403 with a new token', async () => {
  const { runCsrfMutation } = await importTs('./src/api/csrf.ts')
  const mutationTokens = []
  let csrfCount = 0
  let mutationCount = 0

  const fetchImpl = async (path, init = {}) => {
    const headers = init.headers ?? {}
    if (path === '/admin/csrf-token') {
      csrfCount += 1
      return jsonResponse({ csrf_token: `csrf-${csrfCount}` })
    }
    mutationCount += 1
    mutationTokens.push(headers['X-CSRF-Token'])
    if (mutationCount === 1) return jsonResponse('stale csrf', { ok: false, status: 403 })
    return jsonResponse({ ok: true })
  }

  const result = await runCsrfMutation('PUT', '/admin/api/config', '{"a":1}', 'application/json', {
    fetchImpl,
    getToken: () => 'admin-token',
    handleAuthAndErrors: async () => {},
  })

  assert.deepEqual(result, { ok: true })
  assert.deepEqual(mutationTokens, ['csrf-1', 'csrf-2'])
})
