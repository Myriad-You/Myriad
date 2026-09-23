import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { compileFunction } from 'node:vm'
import { act, createElement } from 'react'
import { createRoot } from 'react-dom/client'

const require = createRequire(import.meta.url)
const { JSDOM } = require(require.resolve('jsdom', { paths: [require.resolve('isomorphic-dompurify')] }))
const { build } = createRequire(import.meta.resolve('tsx/package.json'))('esbuild')

test('task polling waits for completion, stops at terminal states, and discards stale work', async (context) => {
  const errors: unknown[][] = []
  context.mock.method(console, 'error', (...args: unknown[]) => errors.push(args))
  const dom = new JSDOM('<div id="root"></div>', { url: 'https://test.invalid' })
  const globals = { window: dom.window, document: dom.window.document, localStorage: dom.window.localStorage, IS_REACT_ACT_ENVIRONMENT: true }
  const previous = new Map(Object.keys(globals).map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]))
  for (const [key, value] of Object.entries(globals)) Object.defineProperty(globalThis, key, { configurable: true, value })
  const timers = new Map<number, { callback: () => void; delay: number }>()
  let nextTimer = 0
  const schedule = (callback: () => void, delay: number) => { timers.set(++nextTimer, { callback, delay }); return nextTimer }
  const requests: { url: string; signal: AbortSignal; resolve: (data: Response) => void; reject: (error: Error) => void }[] = []
  // Stand-in for the service: transport per call, non-2xx rejects with the server message.
  const getTask = async (url: string, signal: AbortSignal) => {
    const response = await new Promise<Response>((resolve, reject) => requests.push({ url, signal, resolve, reject }))
    const body = await response.json()
    if (!response.ok) throw Object.assign(new Error(body.error), { status: response.status })
    return body
  }
  const bundle = await build({
    entryPoints: [fileURLToPath(new URL('./TaskStatus.tsx', import.meta.url))], bundle: true, write: false,
    platform: 'node', format: 'cjs', packages: 'external', define: { 'import.meta.env': '{}' },
    plugins: [{ name: 'boundaries', setup(builder) {
      builder.onResolve({ filter: /(@lib\/icons|contexts\/I18nContext|services\/platformTasksApi|\.\/Spinner)$/ }, ({ path }) => ({ path, external: true }))
    } }],
  })
  const t = { task: { fetchFailed: 'Fetch failed' }, common: {}, reportsPage: {} }
  const mockRequire = (path: string) => {
    if (path.includes('I18nContext')) return { useI18n: () => ({ t, locale: 'en-US' }) }
    if (path === '@lib/icons') return Object.fromEntries(['FaCheckCircle', 'FaExclamationCircle', 'FaSpinner', 'FaTimes'].map(key => [key, () => null]))
    if (path === './Spinner') return { Spinner: () => null }
    if (path.includes('platformTasksApi')) return { getTask }
    return require(path)
  }
  const module = { exports: {} as typeof import('./TaskStatus') }
  compileFunction(bundle.outputFiles[0].text, ['require', 'module', 'exports', 'setTimeout', 'clearTimeout'])(
    mockRequire, module, module.exports, schedule, (id: number) => timers.delete(id),
  )
  const root = createRoot(dom.window.document.getElementById('root'))
  let completed = 0
  let failed = 0
  let closed = 0
  const render = async (taskId = 'a') => act(async () => root.render(createElement(module.exports.TaskStatus, {
    taskId, onComplete: () => { completed++ }, onError: () => { failed++ }, onClose: () => { closed++ },
  })))
  const resolve = async (index: number, status: string, progress = 0) => act(async () => requests[index].resolve(Response.json({
    success: true, task: { id: requests[index].url, platform: 'test', status, progress, created_at: '2026-01-01', updated_at: '2026-01-01' },
  })))
  const tick = async () => {
    const [id, timer] = [...timers.entries()][0]!
    timers.delete(id)
    await act(async () => { timer.callback() })
    return timer.delay
  }
  try {
    await render()
    assert.equal(requests.length, 1)
    assert.equal(timers.size, 0, 'no next poll while a request is outstanding')
    await resolve(0, 'Pending')
    assert.equal(requests.length, 1, 'state updates do not immediately poll again')
    await render()
    assert.equal(requests.length, 1, 'new callback identities do not restart polling')
    assert.equal(await tick(), 1000)
    assert.equal(requests.length, 2)
    assert.equal(timers.size, 0)
    await resolve(1, 'Processing', 60)
    assert.equal(await tick(), 2000, 'next delay uses the latest progress')
    await resolve(2, 'Completed')
    assert.equal(completed, 1)
    await render()
    assert.equal(requests.length, 3)
    assert.equal(await tick(), 3000)
    assert.equal(closed, 1)
    assert.equal(timers.size, 0)
    await render('b')
    assert.equal(requests.length, 4, 'switching task restarts terminal polling')
    await render('c')
    assert.equal(requests[3].signal.aborted, true, 'task switch cancels the old transport')
    assert.equal(requests[4].signal.aborted, false)
    await resolve(3, 'Completed')
    assert.equal(completed, 1, 'stale tasks cannot complete or schedule auto-close')
    assert.equal(timers.size, 0)
    await resolve(4, 'Failed')
    assert.equal(failed, 1)
    assert.equal(timers.size, 0)
    await render('d')
    await resolve(5, 'Completed')
    assert.equal(timers.size, 1)
    await render('e')
    assert.equal(timers.size, 0, 'task switch cancels previous auto-close')
    await render('f')
    await act(async () => requests[6].reject(new DOMException('Aborted', 'AbortError')))
    assert.equal(timers.size, 0, 'cancelled transport does not reschedule the old task')
    assert.equal(errors.length, 0, 'cancelled requests stay silent')
    await act(async () => requests[7].resolve(Response.json({ error: 'Task not found' }, { status: 404 })))
    assert.match(dom.window.document.body.textContent, /Task not found/)
    assert.equal(timers.size, 0, 'HTTP failure stops polling')
    assert.equal(errors.length, 1)
    const sameTask = (key: string) => createElement(module.exports.TaskStatus, { key, taskId: 'same', autoClose: false })
    await act(async () => root.render(createElement('div', null, sameTask('first'), sameTask('second'))))
    assert.equal(requests[8].signal.aborted, false)
    assert.equal(requests[9].signal.aborted, false, 'same task in another component does not cancel this request')
    await act(async () => root.render(createElement('div', null, sameTask('second'))))
    assert.equal(requests[8].signal.aborted, true)
    assert.equal(requests[9].signal.aborted, false, 'one consumer unmount only cancels its own transport')
    await resolve(9, 'Completed')
    await render('g')
    await act(async () => root.unmount())
    assert.equal(requests[10].signal.aborted, true, 'unmount cancels the outstanding transport')
    await resolve(10, 'Completed')
    assert.equal(completed, 2, 'unmounted request cannot invoke callbacks')
    assert.equal(timers.size, 0)
    assert.equal(closed, 1)
  } finally {
    await act(async () => root.unmount())
    dom.window.close()
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor)
      else Reflect.deleteProperty(globalThis, key)
    }
  }
})
