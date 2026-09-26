import assert from 'node:assert/strict'
import test from 'node:test'
import {
  ARM_HANG,
  ARM_MAX_RADIANS,
  ARM_OPEN_RADIANS,
  ARM_SWAY_RADIANS,
  ArmDrape,
  ArmPendulum,
  DRAPE_SAG,
} from './armPendulum'

const STILL = { open: 0, sway: 0, bodyRoll: 0, dynamic: true }
const DT = 1 / 120

function settle(arm: ArmPendulum, input = STILL, seconds = 6, support = { x: 0, y: 0, reach: 400 }) {
  for (let t = 0; t < seconds; t += DT) arm.step(input, support, DT)
  return arm.angle
}

test('opening swings each arm away from the body, and sway carries both one way', () => {
  const left = new ArmPendulum(1)
  const right = new ArmPendulum(-1)
  settle(left, STILL, 0.1)
  settle(right, STILL, 0.1)
  const open = { ...STILL, open: 0.5 }
  assert.ok(Math.abs(settle(left, open) - 0.5 * ARM_OPEN_RADIANS) < 0.01)
  assert.ok(Math.abs(settle(right, open) + 0.5 * ARM_OPEN_RADIANS) < 0.01)
  const sway = { ...STILL, sway: 0.5 }
  assert.ok(Math.abs(settle(left, sway) + 0.5 * ARM_SWAY_RADIANS) < 0.01)
  assert.ok(Math.abs(settle(right, sway) + 0.5 * ARM_SWAY_RADIANS) < 0.01)
})

test('an arm overshoots a sudden change a little and then settles on it', () => {
  const arm = new ArmPendulum(1)
  settle(arm, STILL, 0.1)
  const target = 0.3 * ARM_OPEN_RADIANS
  let peak = 0
  for (let t = 0; t < 3; t += DT) {
    arm.step({ ...STILL, open: 0.3 }, null, DT)
    peak = Math.max(peak, arm.angle)
  }
  assert.ok(peak > target * 1.03 && peak < target * 1.2, `${peak} vs ${target}`)
  assert.ok(Math.abs(settle(arm, { ...STILL, open: 0.3 }) - target) < 0.01)
})

test('a shoulder that starts moving leaves the hand behind for a moment', () => {
  const arm = new ArmPendulum(1)
  settle(arm, STILL, 0.2)
  // The support accelerates toward image right; the hand trails to the left.
  let x = 0
  let v = 0
  let peak = 0
  for (let t = 0; t < 0.4; t += DT) {
    v += 900 * DT
    x += v * DT
    peak = Math.max(peak, arm.step(STILL, { x, y: 0, reach: 400 }, DT))
  }
  assert.ok(peak > 0.02, `${peak}`)
  // Once the shoulder glides at constant speed the arm returns to hanging.
  for (let t = 0; t < 6; t += DT) {
    x += v * DT
    arm.step(STILL, { x, y: 0, reach: 400 }, DT)
  }
  assert.ok(Math.abs(arm.angle) < 1e-3)
})

test('a leaning body lets the hanging arms give some of the roll back', () => {
  const arm = new ArmPendulum(-1)
  settle(arm, STILL, 0.1)
  assert.ok(Math.abs(settle(arm, { ...STILL, bodyRoll: 0.04 }) + ARM_HANG * 0.04) < 1e-3)
})

test('the swing saturates softly and never exceeds its limit', () => {
  const arm = new ArmPendulum(1)
  settle(arm, STILL, 0.1)
  let peak = 0
  for (let t = 0; t < 3; t += DT) peak = Math.max(peak, arm.step({ ...STILL, open: 1, sway: -1 }, null, DT))
  assert.ok(peak < ARM_MAX_RADIANS)
  assert.ok(peak > ARM_MAX_RADIANS * 0.9)
})

test('without dynamics an arm sits exactly on its intent', () => {
  const arm = new ArmPendulum(1)
  const angle = arm.step({ open: 0.3, sway: 0, bodyRoll: 0, dynamic: false }, null, DT)
  assert.ok(Math.abs(angle - ARM_MAX_RADIANS * Math.tanh((0.3 * ARM_OPEN_RADIANS) / ARM_MAX_RADIANS)) < 1e-12)
})

test('the two arms do not swing in lockstep', () => {
  const left = new ArmPendulum(1)
  const right = new ArmPendulum(-1)
  settle(left, STILL, 0.1)
  settle(right, STILL, 0.1)
  let differs = false
  for (let t = 0; t < 1.5; t += DT) {
    const input = { ...STILL, sway: t < 0.2 ? 1 : 0 }
    differs ||= Math.abs(left.step(input, null, DT) - right.step(input, null, DT)) > 1e-3
  }
  assert.ok(differs)
})

test('bad input and a jump in the shoulder cannot throw the arm around', () => {
  const arm = new ArmPendulum(1)
  settle(arm, STILL, 0.2)
  for (const bad of [Number.NaN, Infinity, -Infinity]) {
    const angle = arm.step({ open: bad, sway: bad, bodyRoll: bad, dynamic: true }, { x: bad, y: bad, reach: bad }, bad)
    assert.ok(Number.isFinite(angle))
  }
  settle(arm, STILL, 0.5)
  arm.step(STILL, { x: 0, y: 0, reach: 400 }, DT)
  arm.step(STILL, { x: 0, y: 0, reach: 400 }, DT)
  const jumped = arm.step(STILL, { x: 5000, y: 0, reach: 400 }, DT)
  assert.ok(Math.abs(jumped) < ARM_MAX_RADIANS)
})

test('a drape trails its arm and settles nearer vertical than it', () => {
  const drape = new ArmDrape()
  drape.step(0, 0, true, DT)
  const arm = 0.1
  const sagged = ARM_MAX_RADIANS * Math.tanh((arm * (1 - DRAPE_SAG)) / ARM_MAX_RADIANS)
  let early = 0
  for (let t = 0; t < 0.1; t += DT) early = drape.step(arm, 0, true, DT)
  assert.ok(early > 0 && early < sagged)
  let settled = 0
  for (let t = 0; t < 8; t += DT) settled = drape.step(arm, 0, true, DT)
  assert.ok(Math.abs(settled - sagged) < 1e-4, `${settled}`)
  assert.ok(settled < arm)
})

test('a leaning body pulls the drape back toward vertical', () => {
  const drape = new ArmDrape()
  let angle = 0
  for (let t = 0; t < 8; t += DT) angle = drape.step(0, 0.05, true, DT)
  assert.ok(Math.abs(angle + 0.05 * DRAPE_SAG) < 1e-3)
})

test('a drape without dynamics or with bad input sits on its target', () => {
  const drape = new ArmDrape()
  assert.ok(Math.abs(drape.step(0.1, 0, false, DT) - ARM_MAX_RADIANS * Math.tanh((0.1 * (1 - DRAPE_SAG)) / ARM_MAX_RADIANS)) < 1e-12)
  assert.ok(Number.isFinite(drape.step(Number.NaN, Infinity, true, Number.NaN)))
})
