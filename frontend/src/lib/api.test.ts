import type { Server } from 'node:http'
import assert from 'node:assert/strict'
import { once } from 'node:events'
import { createServer } from 'node:http'
import { after, afterEach, before, beforeEach, describe, it, mock } from 'node:test'
import axios from 'axios'
import { getSiteFace, getWardrobeFace } from '../features/merope/api'
import { ensureSessionStoragePolyfill } from '../test/sessionStoragePolyfill'
import { clearCSRFToken } from '../utils/csrf'
import { RateLimitError } from '../utils/rateLimiter'
import TokenManager from '../utils/tokenManager'
import api, { updateConfig } from './api'

describe('shared HTTP failures', () => {
  let server: Server
  let replies: Array<{ status: number; body: unknown; headers?: Record<string, string> }>
  let calls: number
  const originalBase = api.defaults.baseURL
  const originalWindow = Object.getOwnPropertyDescriptor(globalThis, 'window')
  const originalLocalStorage = Object.getOwnPropertyDescriptor(globalThis, 'localStorage')
  const token = `v1.${'a'.repeat(100)}.${'b'.repeat(43)}`

  before(async () => {
    server = createServer((request, response) => {
      request.resume()
      calls++
      const reply = replies.shift() ?? { status: 500, body: { error: 'Unexpected request' } }
      response.writeHead(reply.status, { 'Content-Type': 'application/json', ...reply.headers })
      response.end(JSON.stringify(reply.body))
    })
    server.listen(0, '127.0.0.1')
    await once(server, 'listening')
    const address = server.address()
    assert.ok(address && typeof address === 'object')
    api.defaults.baseURL = `http://127.0.0.1:${address.port}`
  })

  after(async () => {
    api.defaults.baseURL = originalBase
    server.close()
    await once(server, 'close')
  })

  beforeEach(() => {
    replies = []
    calls = 0
    ensureSessionStoragePolyfill()
    Object.defineProperty(globalThis, 'localStorage', { value: sessionStorage, configurable: true })
    Object.defineProperty(globalThis, 'window', { value: new EventTarget(), configurable: true })
    clearCSRFToken()
    mock.method(globalThis, 'fetch', async () => Response.json({ csrf_token: token, expires_in: 3600 }))
  })

  afterEach(() => {
    clearCSRFToken()
    mock.restoreAll()
    if (originalWindow) Object.defineProperty(globalThis, 'window', originalWindow)
    else Reflect.deleteProperty(globalThis, 'window')
    if (originalLocalStorage) Object.defineProperty(globalThis, 'localStorage', originalLocalStorage)
    else Reflect.deleteProperty(globalThis, 'localStorage')
  })

  for (const status of [400, 403, 404, 409, 422, 500]) {
    it(`rejects HTTP ${status} with the server message and response`, async () => {
      replies.push({ status, body: { error: 'Request rejected by policy', code: 'POLICY_REJECTED' } })
      await assert.rejects(api.get('/read'), (error: unknown) => {
        assert.ok(axios.isAxiosError(error))
        assert.equal(error.response?.status, status)
        assert.equal(error.response?.data.code, 'POLICY_REJECTED')
        assert.equal(error.message, 'Request rejected by policy')
        return true
      })
      assert.equal(calls, 1)
    })
  }

  it('clears authentication and CSRF on 401 before rejecting', async () => {
    const remove = mock.method(TokenManager, 'removeToken', () => {})
    let authEvent: unknown
    window.addEventListener('auth-state-changed', (event) => {
      authEvent = (event as CustomEvent).detail
    })
    sessionStorage.setItem('csrf_token', token)
    replies.push({ status: 401, body: { error: 'Session expired' } })
    await assert.rejects(api.get('/private'), /Session expired/)
    assert.equal(remove.mock.callCount(), 1)
    assert.equal(sessionStorage.getItem('csrf_token'), null)
    assert.deepEqual(authEvent, { isAuthenticated: false })
  })

  it('refreshes CSRF and retries a rejected mutation once', async () => {
    replies.push(
      { status: 403, body: { error: 'Invalid CSRF token' } },
      { status: 200, body: { success: true } },
    )
    assert.deepEqual(await updateConfig({ name: 'Myriad' }), { success: true })
    assert.equal(calls, 2)
  })

  it('rejects a second CSRF failure instead of retrying forever', async () => {
    replies.push(
      { status: 403, body: { error: 'Invalid CSRF token' } },
      { status: 403, body: { error: 'Invalid CSRF token' } },
    )
    await assert.rejects(updateConfig({}), /Invalid CSRF token/)
    assert.equal(calls, 2)
  })

  it('preserves server Retry-After without retrying 429', async () => {
    replies.push({ status: 429, body: {}, headers: { 'Retry-After': '7' } })
    await assert.rejects(api.get('/limited'), (error: unknown) => {
      assert.ok(error instanceof RateLimitError)
      assert.equal(error.retryAfter, 7000)
      return true
    })
    assert.equal(calls, 1)
  })

  it('still rejects a configuration write with success:false in HTTP 200', async () => {
    replies.push({ status: 200, body: { success: false, error: 'Setting is locked' } })
    await assert.rejects(updateConfig({}), /Setting is locked/)
  })

  it('keeps missing site and wardrobe faces as an empty domain result', async () => {
    replies.push({ status: 404, body: {} }, { status: 404, body: {} })
    const empty = { manifest: null, portraitUrl: null, generationFingerprint: null, assetId: null }
    assert.deepEqual(await getSiteFace(), empty)
    assert.deepEqual(await getWardrobeFace('default'), empty)
  })
})
