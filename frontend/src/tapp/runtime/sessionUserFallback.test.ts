/**
 *   pnpm exec tsx --test src/tapp/runtime/sessionUserFallback.test.ts
 */
/* eslint-disable test/no-import-node-test -- node:test */

import type { SessionUserSnapshot } from './sessionUserFallback.ts'

import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import {
  fetchSessionUserSnapshot,
  roleFromSessionSnapshot,
} from './sessionUserFallback.ts'

describe('roleFromSessionSnapshot', () => {
  it('returns guest for null', () => {
    assert.equal(roleFromSessionSnapshot(null), 'guest')
  })

  it('returns user/admin from snapshot', () => {
    const user: SessionUserSnapshot = {
      id: 'user_1',
      username: 'alice',
      isAdmin: false,
      role: 'user',
      authenticated: true,
    }
    assert.equal(roleFromSessionSnapshot(user), 'user')
    assert.equal(
      roleFromSessionSnapshot({ ...user, isAdmin: true, role: 'admin' }),
      'admin',
    )
  })
})

describe('fetchSessionUserSnapshot guest contract', () => {
  const originalFetch = globalThis.fetch

  afterEach(() => {
    globalThis.fetch = originalFetch
  })

  it('returns null for HTTP 200 + authenticated:false (not 401)', async () => {
    globalThis.fetch = (async () =>
      new Response(JSON.stringify({ authenticated: false }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })) as typeof fetch

    const snap = await fetchSessionUserSnapshot()
    assert.equal(snap, null)
  })

  it('returns snapshot for authenticated user body', async () => {
    globalThis.fetch = (async () =>
      new Response(
        JSON.stringify({
          authenticated: true,
          id: 42,
          username: 'carol',
          is_admin: false,
          avatar_url: 'https://example.com/a.png',
        }),
        {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        },
      )) as typeof fetch

    const snap = await fetchSessionUserSnapshot()
    assert.ok(snap)
    assert.equal(snap?.id, 'user_42')
    assert.equal(snap?.username, 'carol')
    assert.equal(snap?.authenticated, true)
    assert.equal(snap?.role, 'user')
  })
})
