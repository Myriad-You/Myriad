import assert from 'node:assert/strict'
import { afterEach, beforeEach, describe, it, mock } from 'node:test'
import { getSiteFace, getWardrobeFace } from '../features/merope/api'
import { ensureSessionStoragePolyfill } from '../test/sessionStoragePolyfill'
import { clearCSRFToken } from '../utils/csrf'
import { HOST_SESSION_RECHECK_EVENT } from '../utils/hostSessionFailure'
import TokenManager from '../utils/tokenManager'
import { ApiError, apiService } from './api'
import { updateConfig } from './configApi'

/** The shared client's failure contract, run against the real modules with a fake backend. */
describe('shared HTTP failures', () => {
  let replies: Array<{ status: number, body: unknown, headers?: Record<string, string> }>
  let sent: Array<{ url: string, init: RequestInit }>
  const originalWindow = Object.getOwnPropertyDescriptor(globalThis, 'window')
  const originalLocalStorage = Object.getOwnPropertyDescriptor(globalThis, 'localStorage')
  const token = `v1.${'a'.repeat(100)}.${'b'.repeat(43)}`

  beforeEach(() => {
    replies = []
    sent = []
    ensureSessionStoragePolyfill()
    Object.defineProperty(globalThis, 'localStorage', { value: sessionStorage, configurable: true })
    Object.defineProperty(globalThis, 'window', {
      value: Object.assign(new EventTarget(), { location: { origin: 'https://myriad.test' } }),
      configurable: true,
    })
    clearCSRFToken()
    mock.method(globalThis, 'fetch', async (input: RequestInfo | URL, init: RequestInit = {}) => {
      const url = String(input)
      if (url.endsWith('/api/csrf-token')) return Response.json({ csrf_token: token, expires_in: 3600 })
      if (url.endsWith('/api/config/public')) return Response.json({ aiAvailability: { image: false } })
      sent.push({ url, init })
      const reply = replies.shift() ?? { status: 500, body: { error: 'Unexpected request' } }
      return Response.json(reply.body, { status: reply.status, headers: reply.headers })
    })
  })

  afterEach(() => {
    clearCSRFToken()
    mock.restoreAll()
    if (originalWindow) Object.defineProperty(globalThis, 'window', originalWindow)
    else Reflect.deleteProperty(globalThis, 'window')
    if (originalLocalStorage) Object.defineProperty(globalThis, 'localStorage', originalLocalStorage)
    else Reflect.deleteProperty(globalThis, 'localStorage')
  })

  it('blocks image generation before dispatch when the image provider is not configured', async () => {
    await assert.rejects(apiService.post('/merope/rig/portrait', {}), (error: unknown) => {
      assert.ok(error instanceof ApiError)
      assert.equal(error.status, 409)
      assert.equal(error.code, 'ai_not_configured')
      return true
    })
    assert.equal(sent.length, 0)
  })

  for (const status of [400, 403, 404, 409, 422, 500]) {
    it(`rejects HTTP ${status} with the server message and body`, async () => {
      replies.push({ status, body: { error: 'Request rejected by policy', code: 'POLICY_REJECTED' } })
      await assert.rejects(apiService.get('/read'), (error: unknown) => {
        assert.ok(error instanceof ApiError)
        assert.equal(error.status, status)
        assert.equal(error.code, 'POLICY_REJECTED')
        assert.equal(error.message, 'Request rejected by policy')
        assert.deepEqual(error.body, { error: 'Request rejected by policy', code: 'POLICY_REJECTED' })
        return true
      })
      assert.equal(sent.length, 1)
    })
  }

  it('requests authoritative session validation on 401 before discarding identity', async () => {
    const remove = mock.method(TokenManager, 'removeToken', () => {})
    let checks = 0
    window.addEventListener(HOST_SESSION_RECHECK_EVENT, () => { checks++ })
    sessionStorage.setItem('csrf_token', token)
    replies.push({ status: 401, body: { error: 'Session expired' } })
    await assert.rejects(apiService.get('/private'), /Session expired/)
    assert.equal(remove.mock.callCount(), 0)
    assert.equal(sessionStorage.getItem('csrf_token'), token)
    assert.equal(checks, 1)
  })

  it('refreshes CSRF and retries a rejected mutation once', async () => {
    replies.push(
      { status: 403, body: { error: 'Invalid CSRF token' } },
      { status: 200, body: { success: true } },
    )
    assert.deepEqual(await updateConfig({ name: 'Myriad' }), { success: true })
    assert.equal(sent.length, 2)
  })

  it('rejects a second CSRF failure instead of retrying forever', async () => {
    replies.push(
      { status: 403, body: { error: 'Invalid CSRF token' } },
      { status: 403, body: { error: 'Invalid CSRF token' } },
    )
    await assert.rejects(updateConfig({}), /Invalid CSRF token/)
    assert.equal(sent.length, 2)
  })

  it('surfaces a long 429 window with its Retry-After instead of waiting', async () => {
    replies.push({ status: 429, body: { error: 'Slow down' }, headers: { 'Retry-After': '60' } })
    await assert.rejects(apiService.get('/limited'), { name: 'ApiError', status: 429, retryAfter: 60 })
    assert.equal(sent.length, 1)
  })

  it('waits out one short 429 window on a read', async () => {
    replies.push(
      { status: 429, body: {}, headers: { 'Retry-After': '0' } },
      { status: 200, body: { ok: true } },
    )
    assert.deepEqual(await apiService.get('/blip'), { ok: true })
    assert.equal(sent.length, 2)
  })

  it('never waits out a 429 on a mutation', async () => {
    replies.push({ status: 429, body: {}, headers: { 'Retry-After': '0' } })
    await assert.rejects(apiService.post('/write', {}), { name: 'ApiError', status: 429 })
    assert.equal(sent.length, 1)
  })

  it('retries a read through transient gateway failures', async () => {
    replies.push({ status: 503, body: {} }, { status: 502, body: {} }, { status: 200, body: { ok: true } })
    assert.deepEqual(await apiService.get('/flaky'), { ok: true })
    assert.equal(sent.length, 3)
  })

  it('never repeats a mutation on a transient failure', async () => {
    replies.push({ status: 503, body: { error: 'Unavailable' } })
    await assert.rejects(apiService.post('/write', { a: 1 }), { name: 'ApiError', status: 503 })
    assert.equal(sent.length, 1)
  })

  it('still rejects a configuration write with success:false in HTTP 200', async () => {
    replies.push({ status: 200, body: { success: false, error: 'Setting is locked' } })
    await assert.rejects(updateConfig({}), /Setting is locked/)
  })

  it('sends multipart bodies untouched so the browser owns the boundary', async () => {
    replies.push({ status: 200, body: { ok: true } })
    const form = new FormData()
    form.append('file', new Blob(['x']), 'x.bin')
    await apiService.post('/upload', form)
    assert.equal(sent[0]!.init.body, form)
    assert.equal((sent[0]!.init.headers as Record<string, string>)['Content-Type'], undefined)
  })

  it('resolves a binary success body as a Blob', async () => {
    replies.push({ status: 200, body: { psd: true } })
    const blob = await apiService.post<Blob>('/binary', {}, { responseType: 'blob' })
    assert.ok(blob instanceof Blob)
    assert.equal(await blob.text(), '{"psd":true}')
  })

  it('keeps missing site and wardrobe faces as an empty domain result', async () => {
    replies.push({ status: 404, body: {} }, { status: 404, body: {} })
    const empty = { manifest: null, portraitUrl: null, generationFingerprint: null, assetId: null }
    assert.deepEqual(await getSiteFace(), empty)
    assert.deepEqual(await getWardrobeFace('default'), empty)
  })
})
