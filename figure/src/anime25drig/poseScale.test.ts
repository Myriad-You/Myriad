import assert from 'node:assert/strict'
import test from 'node:test'
import { angleZForHeadRoll, bodyLeanShare, HEAD_ROLL_RADIANS, HEAD_TILT_RADIANS, headRollRadians } from './poseScale'

test('the canvas cut takes none of the lean and the shoulders take all of it', () => {
  assert.equal(bodyLeanShare(100, 100, 60), 0)
  assert.equal(bodyLeanShare(130, 100, 60), 0)
  assert.equal(bodyLeanShare(40, 100, 60), 1)
  assert.equal(bodyLeanShare(0, 100, 60), 1)
  let previous = 0
  for (let y = 100; y >= 40; y -= 1) {
    const share = bodyLeanShare(y, 100, 60)
    assert.ok(share >= previous)
    previous = share
  }
})

test('no bend height is a rigid lean', () => {
  assert.equal(bodyLeanShare(100, 100, 0), 1)
})

test('everyday head roll stays linear and only a directed tilt reaches the full range', () => {
  for (const angleZ of [0, 0.1, 0.25, 0.45]) {
    assert.ok(Math.abs(headRollRadians(angleZ) - angleZ * HEAD_ROLL_RADIANS) < 1e-12)
  }
  assert.ok(Math.abs(headRollRadians(1) - HEAD_TILT_RADIANS) < 1e-12)
  assert.equal(headRollRadians(-0.8), -headRollRadians(0.8))
  let previous = 0
  for (let angleZ = 0; angleZ <= 1; angleZ += 0.01) {
    const roll = headRollRadians(angleZ)
    assert.ok(roll >= previous)
    assert.ok(roll - previous < 0.02, 'no jump at the knee')
    previous = roll
  }
})

test('the angleZ for a head roll undoes headRollRadians over the whole range', () => {
  for (let angleZ = -1; angleZ <= 1; angleZ += 0.05) {
    assert.ok(Math.abs(angleZForHeadRoll(headRollRadians(angleZ)) - angleZ) < 1e-9, `${angleZ}`)
  }
  assert.equal(angleZForHeadRoll(HEAD_TILT_RADIANS * 2), 1, 'no further than the full tilt')
})
