/**
 * Unit tests for Aro guest-lock role resolution.
 *
 *   pnpm exec tsx --test src/tapp/utils/resolveAroUserRole.test.ts
 */
/* eslint-disable test/no-import-node-test -- node:test; project has no vitest dep */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  isAuthenticatedAroUser,
  resolveAroUserRole,
} from './resolveAroUserRole.ts'

describe('resolveAroUserRole', () => {
  it('defaults to guest when no signals', () => {
    assert.deepEqual(resolveAroUserRole({}), {
      userRole: 'guest',
      isGuest: true,
      isAdmin: false,
    })
  })

  it('honors getRole user/admin; soft-guest alone stays guest without getUser', () => {
    assert.deepEqual(resolveAroUserRole({ roleFromGetRole: 'guest' }), {
      userRole: 'guest',
      isGuest: true,
      isAdmin: false,
    })
    assert.deepEqual(resolveAroUserRole({ roleFromGetRole: 'user' }), {
      userRole: 'user',
      isGuest: false,
      isAdmin: false,
    })
    assert.deepEqual(resolveAroUserRole({ roleFromGetRole: 'admin' }), {
      userRole: 'admin',
      isGuest: false,
      isAdmin: true,
    })
  })

  it('soft-guest from getRole falls through when getUser shows a real member', () => {
    // Host getRole often returns userRole||'guest' when instance.userRole is unset.
    const result = resolveAroUserRole({
      roleFromGetRole: 'guest',
      userFromContext: { id: 'user_1', username: 'alice', role: 'user' },
    })
    assert.equal(result.isGuest, false)
    assert.equal(result.userRole, 'user')
  })

  it('soft-guest from getRole stays guest when context is also guest', () => {
    const result = resolveAroUserRole({
      roleFromGetRole: 'guest',
      userFromContext: { role: 'guest', id: 'guest' },
    })
    assert.equal(result.isGuest, true)
  })

  it('uses isAdmin API when getRole unavailable (false ⇒ logged-in user)', () => {
    assert.deepEqual(resolveAroUserRole({ isAdminFromApi: false }), {
      userRole: 'user',
      isGuest: false,
      isAdmin: false,
    })
    assert.deepEqual(resolveAroUserRole({ isAdminFromApi: true }), {
      userRole: 'admin',
      isGuest: false,
      isAdmin: true,
    })
  })

  it('falls back to getUser when getRole and isAdmin missing', () => {
    assert.deepEqual(
      resolveAroUserRole({
        userFromContext: {
          id: 'user_42',
          username: 'bob',
          role: 'user',
          isAdmin: false,
        },
      }),
      { userRole: 'user', isGuest: false, isAdmin: false },
    )
    assert.deepEqual(
      resolveAroUserRole({
        userFromContext: {
          id: 'user_1',
          username: 'admin',
          role: 'admin',
          isAdmin: true,
        },
      }),
      { userRole: 'admin', isGuest: false, isAdmin: true },
    )
  })

  it('treats empty getRole as unavailable and falls through to getUser', () => {
    const result = resolveAroUserRole({
      roleFromGetRole: '',
      userFromContext: { id: 'user_9', username: 'cara', role: 'user' },
    })
    assert.equal(result.isGuest, false)
    assert.equal(result.userRole, 'user')
  })

  it('stays guest when getUser is missing or explicit guest', () => {
    assert.equal(
      resolveAroUserRole({ userFromContext: null }).isGuest,
      true,
    )
    assert.equal(
      resolveAroUserRole({
        userFromContext: { role: 'guest', authenticated: true },
      }).isGuest,
      true,
    )
  })
})

describe('isAuthenticatedAroUser', () => {
  it('rejects null, guest role, and anonymous ids', () => {
    assert.equal(isAuthenticatedAroUser(null), false)
    assert.equal(isAuthenticatedAroUser({ role: 'guest' }), false)
    assert.equal(isAuthenticatedAroUser({ id: 'user_-1', username: 'g' }), false)
    assert.equal(isAuthenticatedAroUser({ id: 'guest' }), false)
  })

  it('accepts role user/admin, isAdmin, authenticated, or real identity', () => {
    assert.equal(isAuthenticatedAroUser({ role: 'user' }), true)
    assert.equal(isAuthenticatedAroUser({ role: 'admin' }), true)
    assert.equal(isAuthenticatedAroUser({ isAdmin: true }), true)
    assert.equal(isAuthenticatedAroUser({ authenticated: true }), true)
    assert.equal(
      isAuthenticatedAroUser({ id: 'user_3', username: 'alice' }),
      true,
    )
  })
})
