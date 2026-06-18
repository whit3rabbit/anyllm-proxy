import assert from 'node:assert/strict'
import test from 'node:test'
import { resolveConfig } from 'vite'

test('dev server proxies admin API routes to the Rust admin server', async () => {
  process.env.ADMIN_PORT = '3141'
  delete process.env.ANYLLM_ADMIN_PROXY_TARGET

  const config = await resolveConfig({}, 'serve', 'development')
  const proxy = config.server.proxy ?? {}

  assert.ok(proxy['/admin/api'], 'missing /admin/api proxy')
  assert.ok(proxy['/admin/csrf-token'], 'missing /admin/csrf-token proxy')
  assert.ok(proxy['/admin/ws'], 'missing /admin/ws proxy')
  assert.equal(proxy['/admin/api'].target, 'http://127.0.0.1:3141')
  assert.equal(proxy['/admin/ws'].ws, true)
})
