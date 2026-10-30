import assert from 'node:assert/strict'
import test from 'node:test'
import { idleBreathOffset } from './idleBreath'

test('idle breath phase depends only on time, not an authored idle flag', () => {
  const at = idleBreathOffset(3.7)
  assert.equal(
    at.angleX,
    0.13 * Math.sin(3.7 * 0.42) + 0.05 * Math.sin(3.7 * 1.13),
  )
  assert.deepEqual(idleBreathOffset(3.7), at)
  const reusable = { angleX: 0, angleY: 0, angleZ: 0, body: 0 }
  assert.equal(idleBreathOffset(3.7, reusable), reusable)
  assert.deepEqual(reusable, at)
})
