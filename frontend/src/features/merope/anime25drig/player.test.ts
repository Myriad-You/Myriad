import assert from 'node:assert/strict'
import test from 'node:test'
import {
  DEFAULT_FRONT_HAIR_SWAY,
  DEFAULT_REAR_HAIR_SWAY,
  IDENTITY_DRIVER,
  sanitizeDriverPatch,
} from './player'

test('uses restrained front and rear hair sway defaults', () => {
  assert.equal(DEFAULT_FRONT_HAIR_SWAY, 1)
  assert.equal(DEFAULT_REAR_HAIR_SWAY, 0.5)
  assert.equal(IDENTITY_DRIVER.fhAmp, 1)
  assert.equal(IDENTITY_DRIVER.physAmp, 0.5)
})

test('clamps all external driver writes at the runtime boundary', () => {
  const patch = sanitizeDriverPatch({
    angleX: 99,
    mouthOpen: -2,
    armPos: 8,
    bust: Number.NaN,
    talk: true,
  })
  assert.equal(patch.angleX, 1)
  assert.equal(patch.mouthOpen, 0)
  assert.equal(patch.armPos, 1)
  assert.equal(patch.bust, undefined)
  assert.equal(patch.talk, true)
})
