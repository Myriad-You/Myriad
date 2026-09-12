import assert from 'node:assert/strict'
import { afterEach, beforeEach, describe, it, mock } from 'node:test'
import { ensureSessionStoragePolyfill } from '../test/sessionStoragePolyfill'
import {
  clearCSRFToken,
  CSRF_CLIENT_TTL_MS,
  CSRF_TOKEN_MAX_LENGTH,
  CSRF_TOKEN_VERSION,
  getCSRFToken,
  isCsrfCacheFresh,
  isValidCSRFToken,
  parseCsrfTokenResponse,
} from './csrf.ts'

const validToken = `${CSRF_TOKEN_VERSION}.${'a'.repeat(100)}.${'b'.repeat(43)}`

describe('CSRF request ownership', () => {
  beforeEach(() => {
    ensureSessionStoragePolyfill()
    clearCSRFToken()
  })

  afterEach(() => {
    mock.restoreAll()
    clearCSRFToken()
  })

  function tokenResponse(token = validToken) {
    return Response.json({ csrf_token: token, expires_in: 3600 })
  }

  it('shares a cold fetch and caches the token for every waiter', async () => {
    const pending = Promise.withResolvers<Response>()
    const fetch = mock.method(globalThis, 'fetch', () => pending.promise)
    const first = getCSRFToken()
    const second = getCSRFToken()
    pending.resolve(tokenResponse())
    assert.deepEqual(await Promise.all([first, second]), [validToken, validToken])
    assert.equal(sessionStorage.getItem('csrf_token'), validToken)
    assert.equal(await getCSRFToken(), validToken)
    assert.equal(fetch.mock.callCount(), 1)
  })

  it('does not restore a session token after the session was cleared', async () => {
    const pending = Promise.withResolvers<Response>()
    mock.method(globalThis, 'fetch', () => pending.promise)
    const first = getCSRFToken()
    clearCSRFToken()
    pending.resolve(tokenResponse())
    assert.equal(await first, null)
    assert.equal(sessionStorage.getItem('csrf_token'), null)
  })

  for (const oldFinishesFirst of [true, false]) {
    it(`keeps a forced refresh authoritative when the old request finishes ${oldFinishesFirst ? 'first' : 'last'}`, async () => {
      const old = Promise.withResolvers<Response>()
      const fresh = Promise.withResolvers<Response>()
      const nextToken = `${CSRF_TOKEN_VERSION}.${'c'.repeat(100)}.${'d'.repeat(43)}`
      const fetch = mock.method(globalThis, 'fetch', () => old.promise)
      const first = getCSRFToken()
      fetch.mock.mockImplementation(() => fresh.promise)
      const second = getCSRFToken(true)
      if (oldFinishesFirst) {
        old.resolve(tokenResponse())
        assert.equal(await first, null)
      }
      const joined = getCSRFToken()
      assert.equal(fetch.mock.callCount(), 2)
      fresh.resolve(tokenResponse(nextToken))
      assert.deepEqual(await Promise.all([second, joined]), [nextToken, nextToken])
      if (!oldFinishesFirst) {
        old.resolve(tokenResponse())
        assert.equal(await first, nextToken)
      }
      assert.equal(sessionStorage.getItem('csrf_token'), nextToken)
    })
  }
})

describe('parseCsrfTokenResponse', () => {
  it('returns null for guest null token', () => {
    assert.equal(parseCsrfTokenResponse({ csrf_token: null }).token, null)
    assert.equal(
      parseCsrfTokenResponse({ csrf_token: undefined }).token,
      null,
    )
    assert.equal(parseCsrfTokenResponse({}).token, null)
  })

  it('accepts a bounded versioned stateless token', () => {
    const token = validToken
    assert.equal(parseCsrfTokenResponse({ csrf_token: token }).token, token)
  })

  it('parses expires_in seconds for BE-anchored TTL', () => {
    const token = validToken
    const parsed = parseCsrfTokenResponse({
      csrf_token: token,
      expires_in: 1800,
    })
    assert.equal(parsed.token, token)
    assert.equal(parsed.expiresInSec, 1800)
  })

  it('rejects malformed tokens', () => {
    assert.equal(parseCsrfTokenResponse({ csrf_token: 'short' }).token, null)
    assert.equal(
      parseCsrfTokenResponse({
        csrf_token: `${CSRF_TOKEN_VERSION}.payload.${'a'.repeat(42)}`,
      }).token,
      null,
    )
    assert.equal(
      parseCsrfTokenResponse({
        csrf_token: `v2.${'a'.repeat(100)}.${'b'.repeat(43)}`,
      }).token,
      null,
    )
    assert.equal(
      parseCsrfTokenResponse({
        csrf_token: `${CSRF_TOKEN_VERSION}.${'a'.repeat(CSRF_TOKEN_MAX_LENGTH)}.${'b'.repeat(43)}`,
      }).token,
      null,
    )
    assert.equal(parseCsrfTokenResponse({ csrf_token: 123 }).token, null)
  })
})

describe('isValidCSRFToken', () => {
  it('rejects malformed segment alphabets and extra segments', () => {
    assert.equal(isValidCSRFToken(validToken), true)
    assert.equal(
      isValidCSRFToken(`${CSRF_TOKEN_VERSION}.bad=.${'b'.repeat(43)}`),
      false,
    )
    assert.equal(isValidCSRFToken(`${validToken}.extra`), false)
  })
})

describe('isCsrfCacheFresh', () => {
  it('rejects missing / invalid storedAt', () => {
    assert.equal(isCsrfCacheFresh(null), false)
    assert.equal(isCsrfCacheFresh(0), false)
    assert.equal(isCsrfCacheFresh(Number.NaN), false)
  })

  it('is fresh inside ~55min TTL and stale after', () => {
    const now = 1_700_000_000_000
    assert.equal(isCsrfCacheFresh(now - 1000, now), true)
    assert.equal(
      isCsrfCacheFresh(now - (CSRF_CLIENT_TTL_MS - 1), now),
      true,
    )
    assert.equal(isCsrfCacheFresh(now - CSRF_CLIENT_TTL_MS, now), false)
    assert.equal(
      isCsrfCacheFresh(now - CSRF_CLIENT_TTL_MS - 60_000, now),
      false,
    )
  })

  it('prefers absolute expiresAt over storedAt+ttl', () => {
    const now = 1_700_000_000_000
    assert.equal(
      isCsrfCacheFresh(now - 1000, now, CSRF_CLIENT_TTL_MS, now - 1),
      false,
    )
    assert.equal(
      isCsrfCacheFresh(now - 1000, now, CSRF_CLIENT_TTL_MS, now + 60_000),
      true,
    )
  })
})
