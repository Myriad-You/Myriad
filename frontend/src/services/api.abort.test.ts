import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { runInNewContext } from 'node:vm'
import ts from 'typescript'

// Execute the production module with only browser/config/CSRF dependencies replaced.
function harness(csrf: () => Promise<string>, fetcher: typeof fetch) {
  const source = readFileSync(new URL('./api.ts', import.meta.url), 'utf8')
  const js = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText
  const dependencies: Record<string, unknown> = {
    '../config': { API_URL: 'https://example.test' },
    '../i18n/hostLocaleHeaders': { hostLocaleHeaders: () => ({}) },
    '../i18n/localeCopy': { currentCopy: () => ({ errors: { timeout: 'timeout', networkError: 'network' } }) },
    '../utils/aiRequestTimeout.mjs': { aiRequestTimeoutMs: () => 0 },
    '../utils/csrf': { getCSRFToken: csrf, clearCSRFToken: () => {} },
    '../utils/httpRateLimitToast': { notifyHttpRateLimit: () => {} },
    '../utils/userFacingError': { httpStatusMessage: () => 'error' },
  }
  const exports: { apiService?: { post: (url: string, body: unknown, options: { signal: AbortSignal; timeout?: number }) => Promise<unknown> } } = {}
  runInNewContext(js, {
    require: (id: string) => {
      assert.ok(id in dependencies, `unmocked dependency: ${id}`)
      return dependencies[id]
    },
    exports, fetch: fetcher, window: { location: { origin: 'https://example.test' } },
    URL, AbortController, AbortSignal, setTimeout, clearTimeout, Error, console,
  })
  return exports.apiService!
}

test('identity cancellation during CSRF acquisition never dispatches the old POST', async () => {
  const held = Promise.withResolvers<string>()
  const subject = new AbortController()
  let calls = 0
  const api = harness(() => held.promise, async () => { calls++; return new Response('{}') })
  const pending = api.post('/agent/presence', { private: 'A' }, { signal: subject.signal })
  subject.abort()
  held.resolve('token')
  await assert.rejects(pending, { name: 'AbortError' })
  assert.equal(calls, 0)
})

test('in-flight API cancellation reaches fetch and is not misreported as timeout', async () => {
  const entered = Promise.withResolvers<void>()
  const subject = new AbortController()
  const api = harness(async () => 'token', async (_input, options) => {
    const signal = options!.signal!
    entered.resolve()
    return new Promise((_resolve, reject) => {
      signal.addEventListener('abort', () => reject(signal.reason), { once: true })
    })
  })
  const pending = api.post('/agent/presence', {}, { signal: subject.signal })
  await entered.promise
  subject.abort()
  await assert.rejects(pending, { name: 'AbortError' })
})

test('identity change while refreshing a rejected CSRF token prevents retry', async () => {
  const held = Promise.withResolvers<string>()
  const refreshing = Promise.withResolvers<void>()
  const subject = new AbortController()
  let tokens = 0
  let calls = 0
  const api = harness(async () => {
    if (++tokens === 1) return 'old-token'
    refreshing.resolve()
    return held.promise
  }, async () => {
    calls++
    return new Response(JSON.stringify({ error: 'invalid csrf' }), { status: 403 })
  })
  const pending = api.post('/agent/presence', {}, { signal: subject.signal })
  await refreshing.promise
  subject.abort()
  held.resolve('new-token')
  await assert.rejects(pending, { name: 'AbortError' })
  assert.equal(calls, 1)
})

test('the independent request timeout still reports TIMEOUT', async () => {
  const api = harness(async () => 'token', async (_input, options) => new Promise((_resolve, reject) => {
    options!.signal!.addEventListener('abort', () => reject(options!.signal!.reason), { once: true })
  }))
  await assert.rejects(api.post('/agent/presence', {}, {
    signal: new AbortController().signal, timeout: 1,
  }), { code: 'TIMEOUT', status: 408 })
})
