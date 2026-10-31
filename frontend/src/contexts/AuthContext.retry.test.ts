import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import test from 'node:test'
import { compileFunction } from 'node:vm'
import { act, createElement } from 'react'
import { createRoot } from 'react-dom/client'

const require = createRequire(import.meta.url)
const { JSDOM } = require(require.resolve('jsdom', {
  paths: [require.resolve('isomorphic-dompurify')],
}))
const { build } = createRequire(import.meta.resolve('tsx/package.json'))('esbuild')

test('auth probes preserve confirmed identity, retry finitely, and stop on unmount', async () => {
  const dom = new JSDOM('<div id="root"></div>', { url: 'https://myriad.test' })
  const globals = {
    window: dom.window, document: dom.window.document,
    localStorage: dom.window.localStorage, sessionStorage: dom.window.sessionStorage, CustomEvent: dom.window.CustomEvent, IS_REACT_ACT_ENVIRONMENT: true,
  }
  const previous = new Map(Object.keys(globals).map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]))
  for (const [key, value] of Object.entries(globals)) {
    Object.defineProperty(globalThis, key, { configurable: true, value })
  }
  const timers = new Map<number, { callback: () => void; delay: number }>()
  let timerId = 0
  let requests = 0
  const resets: string[] = []
  let response = new Response(JSON.stringify({ authenticated: true, id: 1, username: 'owner', is_admin: true }))
  let networkError = false
  let pendingResolve: ((value: Response) => void) | undefined
  let defer = false
  let lastSignal: AbortSignal | undefined
  const fetchMe = async (_url: string, init?: RequestInit) => {
    lastSignal = init?.signal ?? undefined
    requests++
    if (defer) return new Promise<Response>(resolve => { pendingResolve = resolve })
    if (networkError) throw new TypeError('network unavailable')
    return response.clone()
  }
  const bundle = await build({
    entryPoints: [new URL('./AuthContext.tsx', import.meta.url).pathname],
    bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
    define: { 'import.meta.env': '{}' },
    plugins: [{ name: 'runtime-boundary', setup(builder) {
      builder.onResolve({ filter: /tapp\/(runtime\/Tapp(Scheduler|Runtime)|tappKernel)$/ }, ({ path }) => ({ path, namespace: 'test' }))
      builder.onLoad({ filter: /.*/, namespace: 'test' }, () => ({ contents: 'export const TappScheduler = {reset(){runtimeReset("scheduler")}}; export const tappKernel = {grants: {destroyAll(){runtimeReset("grants")}}}; export const TappRuntime = {reset(){runtimeReset("runtime")}}', loader: 'js' }))
    } }],
  })
  type Auth = ReturnType<typeof import('./AuthContext').useAuth>
  const module = { exports: {} as typeof import('./AuthContext') }
  compileFunction(bundle.outputFiles[0].text, ['require', 'module', 'exports', 'fetch', 'setTimeout', 'clearTimeout', 'runtimeReset'])(
    require, module, module.exports, fetchMe,
    (callback: () => void, delay: number) => { timers.set(++timerId, { callback, delay }); return timerId },
    (id: number) => timers.delete(id),
    (kind: string) => resets.push(kind),
  )
  let auth!: Auth
  function Consumer() { auth = module.exports.useAuth(); return createElement('span', null, auth.isAdmin ? 'admin' : 'guest') }
  const root = createRoot(dom.window.document.getElementById('root'))
  const tick = async () => {
    const [id, timer] = [...timers.entries()][0]!
    timers.delete(id)
    await act(async () => { timer.callback() })
    return timer.delay
  }
  try {
    localStorage.setItem('myriad_session_hint', 'true')
    await act(async () => root.render(createElement(module.exports.AuthProvider, null, createElement(Consumer))))
    assert.equal(auth.isAdmin, true)
    const initialResets = resets.length
    await act(async () => { window.dispatchEvent(new CustomEvent('host-session-recheck')) })
    assert.equal(auth.isAdmin, true, 'an unrelated resource 401 does not log out a valid host session')
    assert.equal(resets.length, initialResets, 'valid session rechecks preserve live TAPP resources')
    networkError = true
    await act(async () => { await auth.checkAuth() })
    assert.equal(auth.isAdmin, true, 'a network error must retain the verified administrator')
    const delays: number[] = []
    for (let i = 0; i < 5 && timers.size; i++) delays.push(await tick())
    assert.deepEqual(delays, [1000, 2000, 4000], 'stop after three automatic retries')
    assert.equal(timers.size, 0)
    networkError = false
    response = new Response('', { status: 503 })
    await act(async () => { await auth.checkAuth() })
    assert.equal(auth.isAdmin, true, '503 must retain the verified administrator')
    for (const status of [401, 403]) {
      const beforeReset = resets.length
      response = new Response('', { status })
      await act(async () => { await auth.checkAuth() })
      assert.equal(auth.isAdmin, false, `${status} must revoke the confirmed identity`)
      assert.deepEqual(resets.slice(beforeReset), ['scheduler', 'grants', 'runtime'], 'expiry uses the same cleanup as explicit logout')
      assert.equal(localStorage.getItem('myriad_session_hint'), null)
      assert.equal(timers.size, 0)
      localStorage.setItem('myriad_session_hint', 'true')
      response = new Response(JSON.stringify({ authenticated: true, id: 1, username: 'owner', is_admin: true }))
      await act(async () => { await auth.checkAuth() })
      assert.equal(auth.isAdmin, true, 'a later successful login restores administrator controls')
    }
    const beforeGuest = resets.length
    response = new Response(JSON.stringify({ authenticated: false }))
    await act(async () => { window.dispatchEvent(new CustomEvent('host-session-recheck')) })
    assert.equal(auth.isAdmin, false)
    assert.deepEqual(resets.slice(beforeGuest), ['scheduler', 'grants', 'runtime'])
    assert.equal(localStorage.getItem('myriad_session_hint'), null)
    assert.equal(timers.size, 0)
    const afterGuest = requests
    await act(async () => { await auth.checkAuth() })
    assert.ok(requests > afterGuest, 'cookie is the session; confirmed guest still probes /api/auth/me')
    localStorage.setItem('myriad_session_hint', 'true')
    networkError = true
    await act(async () => { await auth.checkAuth() })
    assert.equal(timers.size, 1, 'a confirmed result resets the retry budget')
    // A login cleanup resolving after logout cannot re-probe or announce a stale subject.
    const readyEvents: boolean[] = []
    window.addEventListener('tapp-subject-ready', event => {
      readyEvents.push((event as CustomEvent).detail.isAuthenticated)
    })
    const beforeTransition = requests
    await act(async () => {
      window.dispatchEvent(new CustomEvent('auth-state-changed', { detail: { isAuthenticated: true } }))
      window.dispatchEvent(new CustomEvent('auth-state-changed', { detail: { isAuthenticated: false } }))
    })
    assert.deepEqual(readyEvents, [false], 'superseded login must not publish readiness')
    assert.equal(requests, beforeTransition, 'superseded login must not start another probe')
    assert.equal(auth.isAuthenticated, false)

    localStorage.setItem('myriad_session_hint', 'true')
    defer = true
    let stale!: Promise<boolean>
    await act(async () => { stale = auth.checkAuth() })
    const staleSignal = lastSignal
    await act(async () => { auth.logout() })
    assert.equal(staleSignal?.aborted, true, 'logout aborts the outstanding identity request')
    await act(async () => {
      pendingResolve!(new Response(JSON.stringify({ authenticated: true, id: 1, username: 'owner', is_admin: true })))
      await stale
    })
    assert.equal(auth.isAdmin, false, 'even an abort-ignoring transport cannot resurrect the old identity')

    localStorage.setItem('myriad_session_hint', 'true')
    let pending!: Promise<boolean>
    await act(async () => { pending = auth.checkAuth() })
    const queued = auth.checkAuth()
    await act(async () => root.unmount())
    assert.equal(timers.size, 0, 'unmount cancels scheduled retries')
    assert.equal(lastSignal?.aborted, true, 'unmount aborts the outstanding identity request')
    const before = requests
    pendingResolve!(new Response('', { status: 503 }))
    defer = false
    await Promise.all([pending, queued])
    assert.equal(timers.size, 0, 'an in-flight response after unmount cannot schedule more work')
    assert.equal(requests, before)
  } finally {
    await act(async () => root.unmount())
    dom.window.close()
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor)
      else Reflect.deleteProperty(globalThis, key)
    }
  }
})

test('a cookie session is probed even without a localStorage hint', async () => {
  const { JSDOM } = require(require.resolve('jsdom', {
    paths: [require.resolve('isomorphic-dompurify')],
  }))
  const { build } = createRequire(import.meta.resolve('tsx/package.json'))('esbuild')
  const dom = new JSDOM('<div id="root"></div>', { url: 'https://myriad.test' })
  const globals = {
    window: dom.window, document: dom.window.document,
    localStorage: dom.window.localStorage, sessionStorage: dom.window.sessionStorage, CustomEvent: dom.window.CustomEvent, IS_REACT_ACT_ENVIRONMENT: true,
  }
  const previous = new Map(Object.keys(globals).map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]))
  for (const [key, value] of Object.entries(globals)) {
    Object.defineProperty(globalThis, key, { configurable: true, value })
  }
  let requests = 0
  const fetchMe = async () => {
    requests++
    return new Response(JSON.stringify({ authenticated: true, id: 1, username: 'owner', is_admin: true }))
  }
  const bundle = await build({
    entryPoints: [new URL('./AuthContext.tsx', import.meta.url).pathname],
    bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
    define: { 'import.meta.env': '{}' },
    plugins: [{ name: 'runtime-boundary', setup(builder) {
      builder.onResolve({ filter: /tapp\/(runtime\/Tapp(Scheduler|Runtime)|tappKernel)$/ }, ({ path }) => ({ path, namespace: 'test' }))
      builder.onLoad({ filter: /.*/, namespace: 'test' }, () => ({ contents: 'export const TappScheduler = {reset(){}}; export const tappKernel = {grants: {destroyAll(){}}}; export const TappRuntime = {reset(){}}', loader: 'js' }))
    } }],
  })
  type Auth = ReturnType<typeof import('./AuthContext').useAuth>
  const module = { exports: {} as typeof import('./AuthContext') }
  compileFunction(bundle.outputFiles[0].text, ['require', 'module', 'exports', 'fetch', 'setTimeout', 'clearTimeout', 'runtimeReset'])(
    require, module, module.exports, fetchMe, () => 0, () => {}, () => {},
  )
  let auth!: Auth
  function Consumer() { auth = module.exports.useAuth(); return createElement('span') }
  const root = createRoot(dom.window.document.getElementById('root'))
  try {
    localStorage.clear()
    await act(async () => root.render(createElement(module.exports.AuthProvider, null, createElement(Consumer))))
    assert.equal(requests, 1, 'unknown identity must probe /api/auth/me')
    assert.equal(auth.isAdmin, true)
    assert.equal(auth.isAuthenticated, true)
  } finally {
    await act(async () => root.unmount())
    dom.window.close()
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor)
      else Reflect.deleteProperty(globalThis, key)
    }
  }
})
