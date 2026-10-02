import assert from 'node:assert/strict'
import test from 'node:test'
import { ArmChoreography, ELBOW_FLEX, ELBOW_SHARE, FOLLOW_SHARE, WEIGHT_OPEN } from './armChoreography'

const DT = 1 / 60
const NO_ELBOWS = { L: false, R: false }
const BUST = { open: 0, sway: 0, headTurn: 0, weight: null, dynamic: true }

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
