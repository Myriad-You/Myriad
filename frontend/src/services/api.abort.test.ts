import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { runInNewContext } from 'node:vm'
import ts from 'typescript'
import { awaitAbortable } from '../utils/awaitAbortable'

// Execute the production module with only browser/config/CSRF dependencies replaced.
function harness(csrf: () => Promise<string>, fetcher: typeof fetch) {
  const source = readFileSync(new URL('./api.ts', import.meta.url), 'utf8')
  const js = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText
  const dependencies: Record<string, unknown> = {
    '../utils/authSubject': { authSubject: { signal: new AbortController().signal } },
    '../utils/hostSessionFailure': { notifyHostSessionFailure() {} },
    '../config': { API_URL: 'https://example.test' },
    '../i18n/hostLocaleHeaders': { hostLocaleHeaders: () => ({}) },
    '../i18n/localeCopy': { currentCopy: () => ({ errors: { timeout: 'timeout', networkError: 'network' } }) },
    '../utils/aiConfiguration': { fetchWithAiConfiguration: fetcher },
    '../utils/aiRequestTimeout.mjs': { aiRequestTimeoutMs: () => 0 },
    '../utils/csrf': { getCSRFToken: csrf, clearCSRFToken: () => {} },
    '../utils/httpRateLimitToast': { notifyHttpRateLimit: () => {} },
    '../utils/awaitAbortable': { awaitAbortable },
    '../utils/httpStatus': { httpStatusMessage: () => 'error' },
  }
  const exports = {} as { apiService: typeof import('./api').apiService }
  runInNewContext(js, {
    require: (id: string) => {
      assert.ok(Object.hasOwn(dependencies, id), `unmocked dependency: ${id}`)
      return dependencies[id]
    },
    exports, fetch: fetcher, window: { location: { origin: 'https://example.test' } },
    URL, AbortController, AbortSignal, setTimeout, clearTimeout, Error, console,
    FormData, Blob, URLSearchParams,
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

for (const kind of ['json', 'blob'] as const) {
  test(`${kind} response body remains subject to timeout after headers arrive`, async () => {
    const api = harness(async () => 'token', async (_input, options) => {
      const signal = options!.signal!
      return new Response(new ReadableStream({
        start(controller) {
          signal.addEventListener('abort', () => controller.error(signal.reason), { once: true })
        },
      }), { headers: { 'content-type': 'application/json' } })
    })
    const pending = kind === 'json'
      ? api.get('/slow', { timeout: 5 })
      : api.getBlob('/slow', { timeout: 5 })
    const result = await Promise.race([pending.catch(error => error.code), new Promise(resolve => setTimeout(resolve, 50, 'hung'))])
    assert.equal(result, 'TIMEOUT')
  })
}

test('blob body cancellation propagates caller identity loss', async () => {
  const subject = new AbortController()
  const entered = Promise.withResolvers<void>()
  const api = harness(async () => 'token', async (_input, options) => {
    const signal = options!.signal!
    entered.resolve()
    return new Response(new ReadableStream({ start(controller) {
      signal.addEventListener('abort', () => controller.error(signal.reason), { once: true })
    } }))
  })
  const pending = api.getBlob('/slow', { signal: subject.signal, timeout: 30 })
  await entered.promise
  subject.abort()
  await assert.rejects(pending, { name: 'AbortError' })
})

test('cancelling a pending CSRF wait settles immediately without dispatching a POST', async () => {
  const token = Promise.withResolvers<string>()
  const subject = new AbortController()
  let calls = 0
  const api = harness(() => token.promise, async () => { calls++; return Response.json({}) })
  const request = api.post('/agent/presence', {}, { signal: subject.signal })
  subject.abort()
  const result = await Promise.race([
    request.catch(error => error.name),
    new Promise(resolve => setTimeout(resolve, 50, 'hung')),
  ])
  assert.equal(result, 'AbortError')
  token.resolve('late-token')
  await Promise.resolve()
  assert.equal(calls, 0)
})

test('request timeout covers CSRF acquisition, not just the eventual fetch', async () => {
  let calls = 0
  const token = Promise.withResolvers<string>()
  const api = harness(() => token.promise, async () => { calls++; return Response.json({}) })
  const result = await Promise.race([
    api.post('/agent/presence', {}, { timeout: 5 }).catch(error => error.code),
    new Promise(resolve => setTimeout(resolve, 50, 'hung')),
  ])
  assert.equal(result, 'TIMEOUT')
  token.resolve('late-token')
  await Promise.resolve()
  assert.equal(calls, 0)
})

test('timeout while refreshing rejected CSRF does not dispatch a retry', async () => {
  const token = Promise.withResolvers<string>()
  let reads = 0
  let calls = 0
  const api = harness(() => ++reads === 1 ? Promise.resolve('old-token') : token.promise, async () => {
    calls++
    return Response.json({ error: 'invalid csrf' }, { status: 403 })
  })
  const result = await Promise.race([
    api.post('/agent/presence', {}, { timeout: 5 }).catch(error => error.code),
    new Promise(resolve => setTimeout(resolve, 50, 'hung')),
  ])
  assert.equal(result, 'TIMEOUT')
  token.resolve('late-token')
  await Promise.resolve()
  assert.equal(calls, 1)
})
