import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { setKnownAuthState } from '../../utils/authState.ts'
import { fetchSessionUserSnapshot } from './sessionUserFallback.ts'

describe('fetchSessionUserSnapshot guest contract', () => {
  const originalFetch = globalThis.fetch

  afterEach(() => {
    globalThis.fetch = originalFetch
    setKnownAuthState(true)
    try {
      localStorage.removeItem('myriad_session_hint')
    } catch {
    }
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

  it('does not fetch /api/auth/me when the host already knows the viewer is a guest', async () => {
    setKnownAuthState(false)
    let called = 0
    globalThis.fetch = (async () => {
      called++
      return new Response(JSON.stringify({ authenticated: false }), {
        status: 200,
      })
    }) as typeof fetch

    const snap = await fetchSessionUserSnapshot()
    assert.equal(snap, null)
    assert.equal(called, 0)
  })

  it('still fetches when auth state is not a known guest', async () => {
    setKnownAuthState(true)
    let called = 0
    globalThis.fetch = (async () => {
      called++
      return new Response(JSON.stringify({ authenticated: false }), {
        status: 200,
      })
    }) as typeof fetch

    const snap = await fetchSessionUserSnapshot()
    assert.equal(snap, null)
    assert.equal(called, 1)
  })
})
