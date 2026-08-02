/**
 *   pnpm exec tsx --test src/utils/csrf.test.ts
 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  CSRF_CLIENT_TTL_MS,
  isCsrfCacheFresh,
  parseCsrfTokenResponse,
} from './csrf.ts'

describe('parseCsrfTokenResponse', () => {
  it('returns null for guest null token', () => {
    assert.equal(parseCsrfTokenResponse({ csrf_token: null }).token, null)
    assert.equal(
      parseCsrfTokenResponse({ csrf_token: undefined }).token,
      null,
    )
    assert.equal(parseCsrfTokenResponse({}).token, null)
  })

  it('accepts valid 32-char token', () => {
    const token = 'a'.repeat(32)
    assert.equal(parseCsrfTokenResponse({ csrf_token: token }).token, token)
  })

  it('parses expires_in seconds for BE-anchored TTL', () => {
    const token = 'b'.repeat(32)
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
      parseCsrfTokenResponse({ csrf_token: 'x'.repeat(31) }).token,
      null,
    )
    assert.equal(parseCsrfTokenResponse({ csrf_token: 123 }).token, null)
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
    // storedAt looks fresh, but expiresAt already passed
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
