/**
 *   pnpm exec tsx --test src/utils/authMe.test.ts
 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { isAuthMeHttpOk, parseAuthMeResponse } from './authMe.ts'

describe('parseAuthMeResponse', () => {
  it('treats authenticated:false as guest', () => {
    assert.deepEqual(parseAuthMeResponse({ authenticated: false }), {
      authenticated: false,
    })
  })

  it('treats empty/malformed body as guest', () => {
    assert.deepEqual(parseAuthMeResponse(null), { authenticated: false })
    assert.deepEqual(parseAuthMeResponse({}), { authenticated: false })
    assert.deepEqual(parseAuthMeResponse({ authenticated: true }), {
      authenticated: false,
    })
    assert.deepEqual(parseAuthMeResponse({ id: 0, username: 'x' }), {
      authenticated: false,
    })
  })

  it('accepts authenticated user payload', () => {
    const parsed = parseAuthMeResponse({
      authenticated: true,
      id: 7,
      username: 'alice',
      is_admin: true,
      is_owner: true,
      display_name: 'Alice',
    })
    assert.equal(parsed.authenticated, true)
    if (parsed.authenticated) {
      assert.equal(parsed.user.id, 7)
      assert.equal(parsed.user.username, 'alice')
      assert.equal(parsed.user.is_admin, true)
      assert.equal(parsed.user.is_owner, true)
      assert.equal(parsed.user.authenticated, true)
    }
  })

  it('coerces missing is_owner to false (not admin-implied)', () => {
    const parsed = parseAuthMeResponse({
      authenticated: true,
      id: 2,
      username: 'mod',
      is_admin: true,
    })
    assert.equal(parsed.authenticated, true)
    if (parsed.authenticated) {
      assert.equal(parsed.user.is_admin, true)
      assert.equal(parsed.user.is_owner, false)
    }
  })

  it('accepts legacy payload with id but no authenticated flag', () => {
    const parsed = parseAuthMeResponse({
      id: '3',
      username: 'bob',
      is_admin: false,
    })
    assert.equal(parsed.authenticated, true)
    if (parsed.authenticated) {
      assert.equal(parsed.user.id, 3)
      assert.equal(parsed.user.username, 'bob')
    }
  })
})

describe('isAuthMeHttpOk', () => {
  it('accepts 2xx only', () => {
    assert.equal(isAuthMeHttpOk(200), true)
    assert.equal(isAuthMeHttpOk(204), true)
    assert.equal(isAuthMeHttpOk(401), false)
    assert.equal(isAuthMeHttpOk(500), false)
  })
})
