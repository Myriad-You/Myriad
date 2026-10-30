import assert from 'node:assert/strict'
import test from 'node:test'
import {
  ARM_HANG,
  ARM_MAX_RADIANS,
  ARM_OPEN_RADIANS,
  ARM_SWAY_RADIANS,
  ArmDrape,
  ArmPendulum,
  ArmSegment,
  DRAPE_SAG,
  FIGURE_ARM_INERTIA_GAIN,
  FIGURE_ARM_SWING,
  FOREARM,
  HAND,
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

test('a figure\'s drawn shoulder moves as far as its body: its arm is not swung harder for it', () => {
  const kick = (inertiaGain?: number) => {
    const arm = new ArmPendulum(1)
    settle(arm, STILL, 0.2)
    let x = 0
    let v = 0
    let peak = 0
    for (let t = 0; t < 0.4; t += DT) {
      v += 900 * DT
      x += v * DT
      peak = Math.max(peak, arm.step({ ...STILL, inertiaGain }, { x, y: 0, reach: 400 }, DT))
    }
    return peak
  }
  const bust = kick()
  const figure = kick(FIGURE_ARM_INERTIA_GAIN)
  assert.ok(figure > 0 && figure < bust / 3, `${figure} vs ${bust}`)
})

test('slack keeps an arm from being flung away from its body past where it means to be', () => {
  // ArmPendulum(1): positive is away from the body. The shoulder jerks toward
  // the body and back, flinging the hand both ways.
  const swing = (slack?: number) => {
    const arm = new ArmPendulum(1)
    settle(arm, STILL, 0.2)
    let x = 0
    let v = 0
    let away = 0
    let toward = 0
    for (let t = 0; t < 1.2; t += DT) {
      v += (t < 0.15 ? -6000 : t < 0.3 ? 6000 : 0) * DT
      x += v * DT
      const angle = arm.step({ ...STILL, slack }, { x, y: 0, reach: 400 }, DT)
      away = Math.max(away, angle)
      toward = Math.min(toward, angle)
    }
    return { away, toward }
  }
  const slack = 0.02
  const free = swing()
  const held = swing(slack)
  assert.ok(free.away > slack * 2, `${free.away}`)
  assert.ok(held.away <= slack + 1e-9, `${held.away}`)
  assert.ok(Math.abs(held.toward - free.toward) < 1e-3, 'toward the body it swings as freely')
})

test('a figure\'s arms hang plumb as the chest leans: one opens off the body, the other closes in', () => {
  const left = new ArmPendulum(1)
  const right = new ArmPendulum(-1)
  const lean = { ...STILL, ...FIGURE_ARM_SWING, bodyRoll: 0.06 }
  settle(left, { ...STILL, ...FIGURE_ARM_SWING }, 0.1)
  settle(right, { ...STILL, ...FIGURE_ARM_SWING }, 0.1)
  // Plumb: the whole roll given back (softened a little by the swing's limit).
  assert.ok(Math.abs(settle(left, lean) + 0.06) < 4e-3)
  assert.ok(Math.abs(settle(right, lean) + 0.06) < 4e-3)
})

test('a figure\'s hand trails its swaying body, swings past it and settles', () => {
  const arm = new ArmPendulum(1)
  const at = (t: number) => (t < 0.2 ? 0 : 60 * Math.sin(2 * Math.PI * 1.05 * (t - 0.2)))
  let peakHand = 0
  let peakBody = 0
  for (let t = 0; t < 4; t += DT) {
    const x = at(t)
    const angle = arm.step({ ...STILL, ...FIGURE_ARM_SWING }, { x, y: 0, reach: 400 }, DT)
    if (t > 2) {
      peakBody = Math.max(peakBody, Math.abs(x))
      peakHand = Math.max(peakHand, Math.abs(x - 400 * Math.sin(angle)))
    }
  }
  assert.ok(peakHand > peakBody * 1.05 && peakHand < peakBody * 1.6, `${peakHand} vs ${peakBody}`)
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

test('each segment down the arm trails the one above it, quicker and within its joint', () => {
  const forearm = new ArmSegment(FOREARM)
  const hand = new ArmSegment(HAND)
  forearm.step(0, 0, true, DT)
  hand.step(0, 0, true, DT)
  // The upper arm swings out and holds: the forearm and then the hand lag behind it.
  let forearmTurn = 0
  let handTurn = 0
  let handPeak = 0
  for (let i = 0; i < 12; i++) {
    forearmTurn = forearm.step(0.4, 0, true, DT)
    handTurn = hand.step(0.4 + forearmTurn, 0, true, DT)
  }
  assert.ok(forearmTurn < 0 && handTurn < 0, `${forearmTurn} ${handTurn}`)
  for (let i = 0; i < 600; i++) {
    forearmTurn = forearm.step(0.4, 0, true, DT)
    handTurn = hand.step(0.4 + forearmTurn, 0, true, DT)
    handPeak = Math.max(handPeak, Math.abs(handTurn))
  }
  assert.ok(handPeak <= HAND.maxBend + 1e-9)
  // At rest each hangs back towards the vertical by its sag.
  assert.ok(Math.abs(forearmTurn + 0.4 * FOREARM.sag) < 1e-3, `${forearmTurn}`)
  assert.ok(Math.abs(handTurn + (0.4 + forearmTurn) * HAND.sag) < 1e-3, `${handTurn}`)
  // Without physics the hand is set where it hangs, at once.
  const still = new ArmSegment(HAND)
  assert.ok(Math.abs(still.step(0.2, 0, false, DT) + 0.2 * HAND.sag) < 1e-9)
})
