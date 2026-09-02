import type { RandomActionFrame, RandomActionName } from './randomAction'
import assert from 'node:assert/strict'
import test from 'node:test'
import { applyRandomActionFrame, RandomActionController } from './randomAction'

function magnitude(frame: Readonly<RandomActionFrame>): number {
  return Math.max(
    Math.abs(frame.angleX),
    Math.abs(frame.angleY),
    Math.abs(frame.angleZ),
    Math.abs(frame.body),
    Math.abs(frame.brow),
    Math.abs(frame.eyeOpen),
    Math.abs(frame.armY),
    Math.abs(frame.armPos),
  )
}

test('waits briefly, then plays a visible staged action', () => {
  const controller = new RandomActionController(() => 0.5)
  const first = controller.sample(0, true, false)
  assert.equal(magnitude(first), 0)
  assert.equal(controller.getActiveAction(), null)
  assert.equal(magnitude(controller.sample(1.59, true, false)), 0)

  controller.sample(1.61, true, false)
  assert.equal(controller.getActiveAction(), 'shoulderEase')
  const action = controller.sample(2.25, true, false)
  assert.ok(action.armY > 0.05)
  assert.equal(action.brow, 0)
  assert.ok(action.eyeOpen < -0.015)
  assert.ok(action.ambientScale < 0.8)
})

test('reuses one frame object and keeps every action channel bounded', () => {
  let seed = 0x9E37_79B9
  const random = () => {
    seed = (seed * 1_664_525 + 1_013_904_223) >>> 0
    return seed / 0x1_0000_0000
  }
  const controller = new RandomActionController(random)
  const output = controller.sample(0, true, false)
  let previous = { ...output }
  let largestStep = 0
  for (let frame = 1; frame <= 60 * 90; frame += 1) {
    const current = controller.sample(frame / 60, true, false)
    assert.equal(current, output)
    largestStep = Math.max(
      largestStep,
      Math.abs(current.angleX - previous.angleX),
      Math.abs(current.angleY - previous.angleY),
      Math.abs(current.angleZ - previous.angleZ),
      Math.abs(current.armY - previous.armY),
    )
    assert.ok(Math.abs(current.angleX) <= 0.087)
    assert.ok(Math.abs(current.angleY) <= 0.238)
    assert.ok(Math.abs(current.angleZ) <= 0.216)
    assert.ok(Math.abs(current.body) <= 0.141)
    assert.ok(Math.abs(current.brow) <= 0.195)
    assert.ok(Math.abs(current.browAngSym) <= 0.108)
    assert.ok(Math.abs(current.eyeOpen) <= 0.454)
    assert.equal(current.eyeX, 0)
    assert.equal(current.eyeY, 0)
    assert.ok(Math.abs(current.armY) <= 0.346)
    assert.ok(Math.abs(current.armPos) <= 0.108)
    assert.ok(current.ambientScale >= 0.28 && current.ambientScale <= 1)
    previous = { ...current }
  }
  assert.ok(largestStep < 0.025)
})

test('cycles through the complete action catalog without immediate repeats', () => {
  let seed = 0x1234_ABCD
  const random = () => {
    seed = (seed * 1_103_515_245 + 12_345) >>> 0
    return seed / 0x1_0000_0000
  }
  const controller = new RandomActionController(random)
  const seen = new Set<RandomActionName>()
  let activeBefore: RandomActionName | null = null
  let lastStarted: RandomActionName | null = null
  for (let frame = 0; frame <= 60 * 180; frame += 1) {
    controller.sample(frame / 60, true, false)
    const active = controller.getActiveAction()
    if (active && activeBefore === null) {
      assert.notEqual(active, lastStarted)
      seen.add(active)
      lastStarted = active
    }
    activeBefore = active
  }
  assert.deepEqual([...seen].sort(), [
    'headDrift',
    'postureShift',
    'shoulderEase',
    'softBlink',
  ])
})

test('speech or another owner releases an action and resumes without a long freeze', () => {
  const controller = new RandomActionController(() => 0.5)
  controller.sample(0, true, false)
  controller.sample(1.61, true, false)
  const active = { ...controller.sample(2.25, true, false) }
  assert.ok(magnitude(active) > 0.06)

  const releaseStart = { ...controller.sample(2.25, true, true) }
  assert.deepEqual(releaseStart, active)
  assert.ok(magnitude(controller.sample(2.4, true, true)) < magnitude(active))
  assert.ok(magnitude(controller.sample(2.64, true, true)) < 1e-6)
  assert.equal(controller.getActiveAction(), null)

  assert.equal(magnitude(controller.sample(2.64, true, false)), 0)
  assert.equal(magnitude(controller.sample(2.96, true, false)), 0)
  controller.sample(2.98, true, false)
  assert.notEqual(controller.getActiveAction(), null)
})

test('resuming after speech does not snap the head or hands', () => {
  const controller = new RandomActionController(() => 0.5)
  controller.sample(0, true, false)
  controller.sample(1.61, true, false)
  controller.sample(2.25, true, true)
  let previous = { ...controller.sample(2.64, true, false) }
  let largestStep = 0
  for (let frame = 1; frame <= 60; frame += 1) {
    const current = controller.sample(2.64 + frame / 60, true, false)
    largestStep = Math.max(
      largestStep,
      Math.abs(current.angleX - previous.angleX),
      Math.abs(current.angleY - previous.angleY),
      Math.abs(current.angleZ - previous.angleZ),
      Math.abs(current.armY - previous.armY),
    )
    previous = { ...current }
  }
  assert.ok(largestStep < 0.025)
})

test('composes expressions and gestures without reopening authored closed eyes', () => {
  const target = {
    angleX: 0.95,
    angleY: 0,
    angleZ: 0,
    body: 0,
    eyeX: 0,
    eyeY: 0,
    brow: 0.1,
    browAngSym: 0,
    eyeOpenL: 0,
    eyeOpenR: 0.8,
    irisScale: 1,
    armY: 0,
    armPos: 0,
  }
  const frame: RandomActionFrame = {
    angleX: 0.2,
    angleY: 0.1,
    angleZ: 0,
    body: 0.1,
    eyeX: 0.1,
    eyeY: 0,
    brow: 0.2,
    browAngSym: 0.1,
    eyeOpen: 0.1,
    irisScale: -0.05,
    armY: 0.3,
    armPos: -0.1,
    ambientScale: 0.4,
  }
  applyRandomActionFrame(target, frame, 1)

  assert.ok(target.angleX > 0.95 && target.angleX < 1)
  assert.equal(target.eyeOpenL, 0)
  assert.equal(target.eyeOpenR, 0.9)
  assert.ok(Math.abs(target.brow - 0.3) < 1e-12)
  assert.equal(target.armY, 0.3)
  assert.equal(target.armPos, -0.1)
})
