import assert from 'node:assert/strict'
import test from 'node:test'
import { AmbientMotionController } from './ambientMotion'

function magnitude(pose: {
  angleX: number
  angleY: number
  angleZ: number
  body: number
  eyeX: number
  eyeY: number
}): number {
  return Math.max(...Object.values(pose).map(Math.abs))
}

test('starts neutral and waits before selecting its first ambient pose', () => {
  const motion = new AmbientMotionController(() => 0.5)
  assert.equal(magnitude(motion.sample(0, true)), 0)
  assert.equal(magnitude(motion.sample(1.79, true)), 0)
  assert.ok(magnitude(motion.sample(1.81, true)) < 0.001)
  assert.ok(magnitude(motion.sample(2.2, true)) > 0)
})

test('moves the gaze before the head and the body', () => {
  const motion = new AmbientMotionController(() => 0.5)
  motion.sample(0, true)
  motion.sample(1.81, true)
  const early = { ...motion.sample(1.94, true) }
  const eyeTravel = Math.max(Math.abs(early.eyeX), Math.abs(early.eyeY))
  const headTravel = Math.max(
    Math.abs(early.angleX),
    Math.abs(early.angleY),
    Math.abs(early.angleZ),
  )
  assert.ok(eyeTravel > headTravel)
  assert.ok(Math.abs(early.body) < headTravel)
})

test('keeps all generated poses conservative and frame-continuous', () => {
  let seed = 0x1234_5678
  const random = () => {
    seed = (seed * 1_664_525 + 1_013_904_223) >>> 0
    return seed / 0x1_0000_0000
  }
  const motion = new AmbientMotionController(random)
  let previous = { ...motion.sample(0, true) }
  let largestHeadStep = 0
  for (let frame = 1; frame <= 60 * 45; frame += 1) {
    const current = { ...motion.sample(frame / 60, true) }
    largestHeadStep = Math.max(
      largestHeadStep,
      Math.abs(current.angleX - previous.angleX),
      Math.abs(current.angleY - previous.angleY),
      Math.abs(current.angleZ - previous.angleZ),
    )
    assert.ok(Math.abs(current.angleX) <= 0.38)
    assert.ok(Math.abs(current.angleY) <= 0.23)
    assert.ok(Math.abs(current.angleZ) <= 0.145)
    assert.ok(Math.abs(current.body) <= 0.1)
    assert.ok(Math.abs(current.eyeX) <= 0.6)
    assert.ok(Math.abs(current.eyeY) <= 0.34)
    previous = current
  }
  assert.ok(largestHeadStep < 0.02)
})

test('releases an active random pose smoothly when automation is disabled', () => {
  const motion = new AmbientMotionController(() => 0.5)
  motion.sample(0, true)
  motion.sample(1.81, true)
  const active = { ...motion.sample(2.8, true) }
  const releaseStart = { ...motion.sample(2.8, false) }
  assert.deepEqual(releaseStart, active)
  assert.ok(magnitude(motion.sample(3.15, false)) < magnitude(active))
  assert.ok(magnitude(motion.sample(3.7, false)) < 1e-8)
})

test('keeps time-based transitions consistent at 30 and 60 fps', () => {
  const at30 = new AmbientMotionController(() => 0.5)
  const at60 = new AmbientMotionController(() => 0.5)
  let pose30 = { ...at30.sample(0, true) }
  let pose60 = { ...at60.sample(0, true) }
  for (let frame = 1; frame <= 30 * 12; frame += 1) {
    pose30 = { ...at30.sample(frame / 30, true) }
  }
  for (let frame = 1; frame <= 60 * 12; frame += 1) {
    pose60 = { ...at60.sample(frame / 60, true) }
  }
  for (const key of Object.keys(pose30) as Array<keyof typeof pose30>) {
    assert.ok(Math.abs(pose30[key] - pose60[key]) < 0.01)
  }
})
