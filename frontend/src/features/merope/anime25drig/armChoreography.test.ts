import assert from 'node:assert/strict'
import test from 'node:test'
import { ArmChoreography, ELBOW_FLEX, ELBOW_SHARE, FOLLOW_SHARE, resolveDirectedArmIntent, WEIGHT_OPEN } from './armChoreography'
import { ARM_MAX_RADIANS, ArmPendulum, ArmSegment, FOREARM } from './armPendulum'

const DT = 1 / 60
const NO_ELBOWS = { L: false, R: false }
const BUST = { open: 0, sway: 0, headTurn: 0, weight: null, dynamic: true }

test('directed sides replace shared shoulder and elbow recruitment, and reach actual bounded arm physics', () => {
  const automatic = { open: 0.5, bend: 0.2 }
  const left = resolveDirectedArmIntent(automatic, 0.6, 1, 0, 1, 1, true)
  const right = resolveDirectedArmIntent(automatic, 0.6, 0, 0, 1, 1, true)
  assert.deepEqual(left, { open: 1 - ELBOW_SHARE, bend: ELBOW_FLEX, sway: 0 })
  assert.deepEqual(right, { open: 0, bend: 0, sway: 0 })
  const shoulderL = new ArmPendulum(1)
  const shoulderR = new ArmPendulum(-1)
  const forearmL = new ArmSegment(FOREARM)
  const forearmR = new ArmSegment(FOREARM)
  let bendL = 0
  let bendR = 0
  for (let i = 0; i < 120; i++) {
    const angleL = shoulderL.step({ ...left, bodyRoll: 0, dynamic: true }, null, 1 / 120)
    const angleR = shoulderR.step({ ...right, bodyRoll: 0, dynamic: true }, null, 1 / 120)
    bendL = forearmL.step(angleL, 0, true, 1 / 120, left.bend)
    bendR = forearmR.step(angleR, 0, true, 1 / 120, -right.bend)
  }
  assert.ok(shoulderL.angle > 0.1 && shoulderL.angle < ARM_MAX_RADIANS)
  assert.equal(shoulderR.angle, 0)
  assert.ok(bendL > 0.1)
  assert.equal(bendR, 0, 'the still arm must not keep the shared elbow gesture')
  const released = resolveDirectedArmIntent(automatic, 0.6, 0, 0, 0, 0, true)
  assert.deepEqual(released, { ...automatic, sway: 0.6 })
  const sleeve = resolveDirectedArmIntent(automatic, 0.6, 1, 0, 1, 1, false)
  assert.deepEqual(sleeve, { open: 1, bend: 0, sway: 0 }, 'a sleeve without an elbow uses only its real shoulder binding')
})

test('a partial directed arm contribution is not multiplied by its authority twice', () => {
  const automatic = { open: 0.4, bend: 0.2 }
  const authority = 0.5
  // These are the composer's already-weighted drivers, not raw target values.
  const raise = 0.8 * authority
  const swing = -0.6 * authority
  for (const elbow of [false, true]) {
    const intent = resolveDirectedArmIntent(automatic, 0.2, raise, swing, authority, authority, elbow)
    const split = elbow ? 1 - ELBOW_SHARE : 1
    assert.equal(intent.open, automatic.open * (1 - authority) + raise * split)
    assert.equal(intent.bend, automatic.bend * (1 - authority) + (elbow ? raise * ELBOW_FLEX : 0))
    assert.equal(intent.sway, 0.2 * (1 - authority) + swing)
  }
})

function hold(arms: ArmChoreography, intent: Partial<typeof BUST> & { weight?: number | null }, seconds: number, elbows = NO_ELBOWS) {
  for (let t = 0; t < seconds * 60; t++) arms.step({ ...BUST, ...intent }, elbows, DT)
}

test('the arm on the side the head turns to leads a gesture; the other joins later and less', () => {
  const arms = new ArmChoreography()
  hold(arms, {}, 0.5)
  arms.step({ ...BUST, open: 0.8, headTurn: 0.4 }, NO_ELBOWS, DT)
  assert.equal(arms.R.open, 0.8, 'the leading arm lifts at once')
  assert.ok(arms.L.open < 0.05, `${arms.L.open}`)
  hold(arms, { open: 0.8, headTurn: 0.4 }, 3)
  assert.ok(Math.abs(arms.L.open - 0.8 * FOLLOW_SHARE) < 1e-3, `${arms.L.open}`)
  // The lead holds through the gesture even if the head turns away.
  hold(arms, { open: 0.8, headTurn: -0.4 }, 1)
  assert.equal(arms.R.open, 0.8)
  // Once the gesture is over, a head turned the other way leads the next.
  hold(arms, {}, 3)
  arms.step({ ...BUST, open: 0.8, headTurn: -0.4 }, NO_ELBOWS, DT)
  assert.equal(arms.L.open, 0.8)
})

test('with nothing to choose by, gestures alternate arms; without dynamics the follower is set at once', () => {
  const arms = new ArmChoreography()
  hold(arms, { open: 0.5 }, 0.2)
  const first = arms.R.open === 0.5 ? 'R' : 'L'
  hold(arms, {}, 2)
  hold(arms, { open: 0.5 }, 0.2)
  assert.equal(arms[first === 'R' ? 'L' : 'R'].open, 0.5)
  const still = new ArmChoreography()
  still.step({ ...BUST, open: 0.5, headTurn: 0.3, dynamic: false }, NO_ELBOWS, DT)
  assert.equal(still.R.open, 0.5)
  assert.equal(still.L.open, 0.5 * FOLLOW_SHARE)
  // Small symmetric lifts are not gestures: both arms take them whole.
  const small = new ArmChoreography()
  small.step({ ...BUST, open: 0.04, dynamic: false }, NO_ELBOWS, DT)
  assert.equal(small.L.open, 0.04)
  assert.equal(small.R.open, 0.04)
})

test('an arm with an elbow lifts partly at the elbow; the hip under the weight pushes its arm out', () => {
  const arms = new ArmChoreography()
  arms.step({ ...BUST, open: 1, headTurn: 0.5, dynamic: false }, { L: false, R: true }, DT)
  assert.ok(Math.abs(arms.R.open - (1 - ELBOW_SHARE)) < 1e-9)
  assert.equal(arms.R.bend, ELBOW_FLEX)
  assert.equal(arms.L.bend, 0)
  const standing = new ArmChoreography()
  standing.step({ ...BUST, weight: 1, dynamic: false }, NO_ELBOWS, DT)
  assert.equal(standing.R.open, WEIGHT_OPEN)
  assert.equal(standing.L.open, 0)
  standing.step({ ...BUST, weight: -0.5, dynamic: false }, NO_ELBOWS, DT)
  assert.equal(standing.L.open, WEIGHT_OPEN * 0.5)
  assert.equal(standing.R.open, 0)
})
