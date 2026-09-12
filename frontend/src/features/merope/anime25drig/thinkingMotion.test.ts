import assert from 'node:assert/strict'
import test from 'node:test'
import { ThinkingMotionController } from './thinkingMotion'

function sequence(...values: number[]) {
  let index = 0
  return () => values[index++ % values.length] ?? 0.5
}

function magnitude(pose: ReturnType<ThinkingMotionController['sample']>) {
  return Math.max(
    Math.abs(pose.angleX),
    Math.abs(pose.angleY),
    Math.abs(pose.angleZ),
    Math.abs(pose.eyeX),
    Math.abs(pose.eyeY),
    Math.abs(pose.brow),
    Math.abs(pose.mouthCY),
    Math.abs(pose.mouthCAng),
    Math.abs(pose.mouthScale),
  )
}

test('thinking eyes refocus before the head follows', () => {
  const motion = new ThinkingMotionController(sequence(0, 0.5, 1, 0.25, 0.75))
  assert.equal(magnitude(motion.sample(0, true)), 0)
  assert.equal(magnitude(motion.sample(0.17, true)), 0)

  const eyeLead = { ...motion.sample(0.23, true) }
  assert.ok(Math.abs(eyeLead.eyeX) > 0)
  assert.equal(eyeLead.angleX, 0)
  assert.equal(eyeLead.angleZ, 0)

  const headFollow = { ...motion.sample(0.5, true) }
  assert.ok(Math.abs(headFollow.eyeX) > 0.1)
  assert.ok(Math.abs(headFollow.angleX) > 0)
  assert.ok(Math.abs(headFollow.angleZ) > 0)
  assert.ok(Math.abs(headFollow.mouthScale) > 0)
})

test('thinking motion stays constrained and releases smoothly', () => {
  const motion = new ThinkingMotionController(() => 0.5)
  motion.sample(0, true)
  for (let frame = 1; frame <= 360; frame += 1) {
    const pose = motion.sample(frame / 60, true)
    assert.ok(Math.abs(pose.eyeX) <= 0.27)
    assert.ok(Math.abs(pose.eyeY) <= 0.13)
    assert.ok(Math.abs(pose.angleX) <= 0.145)
    assert.ok(Math.abs(pose.angleY) <= 0.145)
    assert.ok(Math.abs(pose.angleZ) <= 0.135)
    assert.ok(Math.abs(pose.brow) <= 0.055)
    assert.ok(Math.abs(pose.mouthCY) <= 0.035)
    assert.ok(Math.abs(pose.mouthCAng) <= 0.06)
    assert.ok(Math.abs(pose.mouthScale) <= 0.075)
  }

  const active = { ...motion.sample(6, true) }
  const releaseStart = { ...motion.sample(6, false) }
  assert.deepEqual(releaseStart, active)
  assert.ok(magnitude(motion.sample(6.25, false)) < magnitude(active))
  assert.ok(magnitude(motion.sample(6.7, false)) < 1e-8)
})

test('holds each thought point instead of shifting continuously', () => {
  const motion = new ThinkingMotionController(() => 0.5)
  for (let frame = 0; frame <= 120; frame++) motion.sample(frame / 60, true)
  const settled = { ...motion.sample(2, true) }
  const held = { ...motion.sample(3.4, true) }
  assert.deepEqual(held, settled)
  let checkedBack = false
  for (let frame = 205; frame <= 900; frame++) {
    const pose = motion.sample(frame / 60, true)
    checkedBack ||= Math.abs(pose.eyeX) < 0.01
  }
  assert.ok(checkedBack, 'a long thought occasionally acknowledges the user')
})

test('gaze changes do not flip the thinking head from side to side', () => {
  for (const fps of [30, 60, 120]) {
    const motion = new ThinkingMotionController(sequence(0, 1, 0.3, 0.8, 0.5))
    let previous = { ...motion.sample(0, true) }
    let left = false
    let right = false
    for (let frame = 1; frame <= fps * 30; frame++) {
      const pose = motion.sample(frame / fps, true)
      assert.ok(pose.angleX <= 0 && pose.angleZ <= 0)
      assert.ok(Math.abs(pose.angleX - previous.angleX) * fps < 0.2)
      assert.ok(Math.abs(pose.angleZ - previous.angleZ) * fps < 0.2)
      left ||= pose.eyeX < -0.1
      right ||= frame > fps * 2 && Math.abs(pose.eyeX) < 0.02
      previous = { ...pose }
    }
    assert.ok(
      left && right,
      'eyes look away and check back without alternating sides',
    )
    const delayed = { ...motion.sample(60, true) }
    assert.deepEqual(
      delayed,
      previous,
      'a delayed frame cannot skip into a new thought',
    )
  }
})

test('keeps gaze, head, and mouth curves continuous at 60 fps', () => {
  const motion = new ThinkingMotionController(() => 0.5)
  let previous = { ...motion.sample(0, true) }
  for (let frame = 1; frame <= 360; frame += 1) {
    const current = { ...motion.sample(frame / 60, true) }
    assert.ok(Math.abs(current.eyeX - previous.eyeX) < 0.05)
    assert.ok(Math.abs(current.eyeY - previous.eyeY) < 0.035)
    assert.ok(Math.abs(current.angleX - previous.angleX) < 0.02)
    assert.ok(Math.abs(current.angleY - previous.angleY) < 0.02)
    assert.ok(Math.abs(current.angleZ - previous.angleZ) < 0.02)
    assert.ok(Math.abs(current.mouthScale - previous.mouthScale) < 0.01)
    previous = current
  }
})

test('sampling reuses its output object', () => {
  const motion = new ThinkingMotionController(() => 0.5)
  assert.equal(motion.sample(0, true), motion.sample(1, true))
})

test('check-ins counter the authored gaze and answer handoff starts immediately', () => {
  const motion = new ThinkingMotionController(() => 0.5)
  const base = { eyeX: 0.58, eyeY: -0.42 }
  let checked = false
  let returned = false
  for (let frame = 0; frame <= 900; frame++) {
    const pose = motion.sample(frame / 60, true, base)
    if (
      frame > 60 &&
      Math.abs(base.eyeX + pose.eyeX) < 0.02 &&
      Math.abs(base.eyeY + pose.eyeY) < 0.02
    ) {
      checked = true
    }
    if (checked && base.eyeX + pose.eyeX > 0.3) returned = true
  }
  assert.ok(checked && returned)
  const before = { ...motion.sample(15, true, base) }
  assert.deepEqual(motion.sample(15, false, base), before)
  assert.ok(magnitude(motion.sample(15.1, false)) < magnitude(before))
  assert.ok(magnitude(motion.sample(15.7, false)) < 1e-8)
})
