import assert from 'node:assert/strict'
import test from 'node:test'
import { MinimumJerkMotion } from './minimumJerk'

test('a rest-to-rest movement has exact position, velocity and acceleration endpoints', () => {
  const motion = new MinimumJerkMotion()
  motion.retarget(0, 0.8, 1)
  assert.equal(motion.sample(0), 0)
  assert.equal(motion.velocity, 0)
  assert.equal(motion.acceleration, 0)
  assert.ok(Math.abs(motion.sample(0.5) - 0.4) < 1e-10)
  assert.equal(motion.sample(1), 0.8)
  assert.equal(motion.velocity, 0)
  assert.equal(motion.acceleration, 0)
})

test('mid-motion retargeting inherits position, velocity and acceleration instead of restarting', () => {
  const motion = new MinimumJerkMotion()
  motion.retarget(0, 0.8, 1)
  motion.sample(0.4)
  const before = [motion.value, motion.velocity, motion.acceleration]
  motion.retarget(0.4, -0.7, 1.3, 0.1)
  motion.sample(0.4)
  assert.deepEqual([motion.value, motion.velocity, motion.acceleration], before)
  assert.equal(motion.sample(1.7), -0.7)
  assert.equal(motion.velocity, 0)
  assert.equal(motion.acceleration, 0)
})

test('a stationary head can lag the eyes without starting its trajectory early', () => {
  const motion = new MinimumJerkMotion()
  motion.retarget(0, 1, 1, 0.1)
  assert.equal(motion.sample(0.09), 0)
  assert.ok(motion.sample(0.3) > 0)
})
