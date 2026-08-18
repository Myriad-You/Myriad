import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import {
  applyFederationLimitsPayload,
  DEFAULT_FEDERATION_LIVE_LIMITS,
  federationLiveLimits,
  federationMessageEnvelopeBytes,
  resetFederationLimitsForTests,
} from './federationLimits.ts'

describe('federationLimits', () => {
  afterEach(() => {
    resetFederationLimitsForTests()
  })

  it('defaults match the bounded product profile', () => {
    assert.equal(federationLiveLimits().messagePayloadBytes, 4 * 1024 * 1024)
    assert.equal(federationLiveLimits().noteImageBytes, 32 * 1024 * 1024)
    assert.equal(federationLiveLimits().noteVideoBytes, 256 * 1024 * 1024)
    assert.equal(
      federationMessageEnvelopeBytes(),
      DEFAULT_FEDERATION_LIVE_LIMITS.messagePayloadBytes + 64 * 1024,
    )
  })

  it('applies saver caps from the public limits payload', () => {
    const applied = applyFederationLimitsPayload({
      profile: 'saver',
      message_payload_bytes: 2 * 1024 * 1024,
      note_image_bytes: 8 * 1024 * 1024,
      note_video_bytes: 32 * 1024 * 1024,
    })
    assert.ok(applied)
    assert.equal(applied.profile, 'saver')
    assert.equal(applied.messagePayloadBytes, 2 * 1024 * 1024)
    assert.equal(applied.noteImageBytes, 8 * 1024 * 1024)
    assert.equal(applied.noteVideoBytes, 32 * 1024 * 1024)
    assert.equal(federationMessageEnvelopeBytes(), 2 * 1024 * 1024 + 64 * 1024)
  })

  it('rejects incomplete payloads and keeps defaults', () => {
    assert.equal(applyFederationLimitsPayload({ profile: 'saver' }), null)
    assert.deepEqual(federationLiveLimits(), DEFAULT_FEDERATION_LIVE_LIMITS)
  })
})
