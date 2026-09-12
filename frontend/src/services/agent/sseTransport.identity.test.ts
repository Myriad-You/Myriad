import type * as Transport from './sseTransport'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { runInNewContext } from 'node:vm'
import ts from 'typescript'
import { AuthSubjectScope } from '../../utils/authSubject'
import { ApiError, parseApiErrorBody } from '../api'
import * as turnIdentity from './turnIdentity'

const code = ts.transpileModule(readFileSync(new URL('./sseTransport.ts', import.meta.url), 'utf8'), {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText
const final = { success: true, message: 'done', responseType: 'task_completed', suggestions: [] }
function stream(events: unknown[]): Response {
  return new Response(events.map(event => `data: ${JSON.stringify(event)}\n\n`).join(''), {
    headers: { 'content-type': 'text/event-stream' },
  })
}
function harness(csrf: () => Promise<string>, fetcher: typeof fetch) {
  const subject = new AuthSubjectScope()
  const dependencies: Record<string, unknown> = {
    '../../i18n/hostLocaleHeaders': { hostLocaleHeaders: () => ({}) },
    '../../i18n/localeCopy': { currentCopy: () => ({ errors: {} }) },
    '../../utils/authSubject': { authSubject: subject },
    '../../utils/csrf': { getCSRFToken: csrf, clearCSRFToken: () => {} },
    '../../utils/userFacingError': { isUselessErrorText: () => false },
    '../api': { ApiError, parseApiErrorBody },
    './taskEnvelope': { messageFromStepOutput: () => 'done' },
    './turnIdentity': turnIdentity,
  }
  const exports = {} as typeof Transport
  runInNewContext(code, {
    exports,
    require: (id: string) => { assert.ok(id in dependencies, id); return dependencies[id] },
    AbortController, AbortSignal, setTimeout, clearTimeout, TextDecoder, Error, console,
    fetch: fetcher,
  })
  const activeControllers = new Set<AbortController>()
  const options = {
    url: '/api/agent/process/stream', method: 'POST' as const,
    body: { input: 'private A' }, abortPrevious: false, activeControllers,
    pollTaskUntilComplete: async (): Promise<never> => { throw new Error('unexpected poll') },
  }
  return { ...exports, options, subject, activeControllers }
}

test('identity abort owns CSRF preparation and rejects before the token resolves', async () => {
  const held = Promise.withResolvers<string>()
  let tokens = 0
  let posts = 0
  const h = harness(() => ++tokens === 1 ? held.promise : Promise.resolve('B token'), async () => {
    posts++
    return stream([{ type: 'task_completed', response: final }])
  })
  const pending = h.executeSSERequest(h.options)
  assert.equal(h.activeControllers.size, 1)
  await Promise.resolve()
  h.subject.change('B')
  await assert.rejects(pending, /interrupted by user/)
  assert.equal(h.activeControllers.size, 0)
  await h.executeSSERequest({ ...h.options, body: { input: 'B input' } })
  held.resolve('old token')
  await new Promise(resolve => setImmediate(resolve))
  assert.equal(posts, 1)
})

test('manual cancellation and replacement also cancel CSRF preparation', async () => {
  for (const intent of ['user', 'replace'] as const) {
    const held = Promise.withResolvers<string>()
    let posts = 0
    const h = harness(() => held.promise, async () => { posts++; return stream([]) })
    const pending = h.executeSSERequest(h.options)
    await Promise.resolve()
    h.abortSseSubscriptions(h.activeControllers, intent)
    await assert.rejects(pending, intent === 'user' ? /interrupted by user/ : /superseded/i)
    held.resolve('token')
    await new Promise(resolve => setImmediate(resolve))
    assert.equal(posts, 0)
  }
})

test('a stale submitting signal cannot cancel or replace a current request', async () => {
  const h = harness(async () => 'token', async () => stream([]))
  const old = h.subject.signal
  h.subject.change('B')
  const current = new AbortController()
  h.activeControllers.add(current)
  await assert.rejects(h.executeSSERequest({ ...h.options, signal: old, abortPrevious: true }), { name: 'AbortError' })
  assert.equal(current.signal.aborted, false)
})

test('CSRF refresh after 403 cannot re-POST after identity loss', async () => {
  const held = Promise.withResolvers<string>()
  const refreshing = Promise.withResolvers<void>()
  let tokens = 0
  let posts = 0
  const h = harness(async () => {
    if (++tokens === 1) return 'token'
    refreshing.resolve()
    return held.promise
  }, async () => { posts++; return new Response('invalid csrf', { status: 403 }) })
  const pending = h.executeSSERequest(h.options)
  await refreshing.promise
  h.subject.change('B')
  await assert.rejects(pending, /interrupted by user/)
  held.resolve('new token')
  await new Promise(resolve => setImmediate(resolve))
  assert.equal(posts, 1)
})

test('transport EOF still resumes a known run for the same identity', async () => {
  const urls: string[] = []
  const h = harness(async () => 'token', async url => {
    urls.push(String(url))
    return urls.length === 1
      ? stream([{ type: 'run_started', runId: 'run-1' }])
      : stream([{ type: 'task_completed', response: final }])
  })
  assert.equal((await h.executeSSERequest(h.options)).message, 'done')
  assert.deepEqual(urls, ['/api/agent/process/stream', '/api/agent/runs/run-1/stream'])
  assert.equal(h.activeControllers.size, 0)
})

test('a cancelled polling fallback cannot publish late progress or completion', async () => {
  const entered = Promise.withResolvers<AbortSignal>()
  const held = Promise.withResolvers<void>()
  let progress = 0
  const h = harness(async () => 'token', async () => stream([{ type: 'task_created', taskId: 'task-1' }]))
  const pending = h.executeSSERequest({
    ...h.options,
    onProgress: () => { progress++ },
    pollTaskUntilComplete: async (_id, options) => {
      entered.resolve(options.signal!)
      await held.promise
      options.onProgress?.({ status: 'completed', progress: 100 } as never)
      return { status: 'completed' } as never
    },
  })
  const signal = await entered.promise
  h.abortSseSubscriptions(h.activeControllers)
  await assert.rejects(pending, /interrupted by user/)
  assert.equal(signal.aborted, true)
  held.resolve()
  await new Promise(resolve => setImmediate(resolve))
  assert.equal(progress, 1)
})
