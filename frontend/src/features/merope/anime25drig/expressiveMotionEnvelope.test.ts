import assert from 'node:assert/strict'
import test from 'node:test'
import {
  expressiveEyeOpenOffset,
  semanticRollMotionOffset,
  semanticVerticalMotionOffset,
  speechBrowMotionOffset,
  speechHeadMotionOffset,
} from './expressiveMotionEnvelope'

test('maps semantic and co-speech accents into composable offsets', () => {
  assert.ok(Math.abs(semanticVerticalMotionOffset(0.2) - 0.02) < 1e-12)
  assert.ok(Math.abs(semanticRollMotionOffset(-0.2) - -0.036) < 1e-12)
  assert.ok(Math.abs(speechHeadMotionOffset(0.2) - 0.03) < 1e-12)
  assert.ok(Math.abs(speechBrowMotionOffset(0.2) - 0.024) < 1e-12)
  assert.ok(
    Math.abs(
      expressiveEyeOpenOffset({ brow: 0, eyeOpen: -0.2, angleY: 0 }) -
        -0.024,
    ) < 1e-12,
  )
})

test('invalid expressive signals collapse to neutral offsets', () => {
  assert.equal(semanticVerticalMotionOffset(Number.NaN), 0)
  assert.equal(semanticRollMotionOffset(Number.POSITIVE_INFINITY), 0)
  assert.equal(speechHeadMotionOffset(Number.NEGATIVE_INFINITY), 0)
  assert.equal(speechBrowMotionOffset(Number.NaN), 0)
  assert.equal(
    expressiveEyeOpenOffset({
      brow: Number.NaN,
      eyeOpen: Number.POSITIVE_INFINITY,
      angleY: Number.NEGATIVE_INFINITY,
    }),
    0,
  )
})
