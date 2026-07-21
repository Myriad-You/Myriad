/**
 *   pnpm exec tsx --test src/utils/csrf.test.ts
 */
/* eslint-disable test/no-import-node-test -- node:test */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { parseCsrfTokenResponse } from './csrf.ts'

describe('parseCsrfTokenResponse', () => {
  it('returns null for guest null token', () => {
    assert.equal(parseCsrfTokenResponse({ csrf_token: null }), null)
    assert.equal(parseCsrfTokenResponse({ csrf_token: undefined }), null)
    assert.equal(parseCsrfTokenResponse({}), null)
  })

  it('accepts valid 32-char token', () => {
    const token = 'a'.repeat(32)
    assert.equal(parseCsrfTokenResponse({ csrf_token: token }), token)
  })

  it('rejects malformed tokens', () => {
    assert.equal(parseCsrfTokenResponse({ csrf_token: 'short' }), null)
    assert.equal(parseCsrfTokenResponse({ csrf_token: 'x'.repeat(31) }), null)
    assert.equal(parseCsrfTokenResponse({ csrf_token: 123 }), null)
  })
})
