import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { shouldFetchLoginOnly } from './authGate.ts'

describe('shouldFetchLoginOnly', () => {
  it('skips login-only fetches for a known guest without a session hint', () => {
    assert.equal(
      shouldFetchLoginOnly({ isKnownGuest: true, hasSessionHint: false }),
      false,
    )
    assert.equal(shouldFetchLoginOnly({ isKnownGuest: true }), false)
  })

  it('fails closed on the old guest /api/auth/me probe', () => {
    const wouldHitAuthMe = shouldFetchLoginOnly({
      isKnownGuest: true,
      hasSessionHint: false,
    })
    assert.equal(
      wouldHitAuthMe,
      false,
      'known guests with no session hint must not fetch /api/auth/me',
    )
  })

  it('still probes when a session hint exists (login just happened)', () => {
    assert.equal(
      shouldFetchLoginOnly({ isKnownGuest: true, hasSessionHint: true }),
      true,
    )
  })

  it('probes when the host has not confirmed a guest', () => {
    assert.equal(
      shouldFetchLoginOnly({ isKnownGuest: false, hasSessionHint: false }),
      true,
    )
    assert.equal(
      shouldFetchLoginOnly({ isKnownGuest: false, hasSessionHint: true }),
      true,
    )
  })
})
