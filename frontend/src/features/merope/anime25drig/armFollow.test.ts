import assert from 'node:assert/strict'
import test from 'node:test'
import {
  ARM_SWING_LIFT,
  ArmFollowController,
  FULL_SLIP_RADIANS,
} from './armFollow'

/** Torso yaw held still, then turned over `turnSeconds`, then held again. */
function turn(
  follow: ArmFollowController,
  turnSeconds: number,
  yawRadians: number,
  totalSeconds: number,
  fps = 60,
  limit = 1,
): { at: (seconds: number) => number; peak: number } {
  const dt = 1 / fps
  const samples: number[] = []
  let peak = 0
  for (let index = 0; index <= Math.round(totalSeconds / dt); index += 1) {
    const time = index * dt
    const yaw =
      time >= turnSeconds ? yawRadians : (time / turnSeconds) * yawRadians
    const state = follow.step(yaw, dt, limit)
    samples.push(state.swing)
    peak = Math.max(peak, Math.abs(state.swing))
  }
  return {
    at: (seconds) => samples[Math.round(seconds / dt)] ?? 0,
    peak,
  }
}

test('a torso that is not turning leaves the sleeves alone', () => {
  const follow = new ArmFollowController()
  for (let index = 0; index < 240; index += 1) {
    const state = follow.step(0.31, 1 / 60, 1)
    assert.equal(state.swing, 0)
    assert.equal(state.lift, 0)
  }
})

test('the sleeve falls behind a turn and then closes the gap', () => {
  const swept = turn(new ArmFollowController(), 0.3, 0.45, 3)
  // Against the turn while it happens: the sleeve is still where the torso was.
  assert.ok(swept.at(0.1) < 0, `${swept.at(0.1)}`)
  assert.ok(swept.at(0.2) < swept.at(0.1), 'lag stopped opening mid-turn')
  assert.ok(swept.at(0.3) < -0.1, `${swept.at(0.3)}`)
  // A lag, not an offset: once the torso stands still, the sleeve arrives.
  assert.ok(swept.at(0.6) > swept.at(0.3), 'lag stopped closing after the turn')
  assert.ok(Math.abs(swept.at(1.2)) < Math.abs(swept.peak) * 0.25)
  assert.ok(Math.abs(swept.at(3)) < 0.01, `${swept.at(3)}`)
})

test('a turn the other way lags the other way', () => {
  const left = turn(new ArmFollowController(), 0.3, -0.45, 1)
  const right = turn(new ArmFollowController(), 0.3, 0.45, 1)
  assert.ok(left.at(0.45) > 0)
  assert.ok(right.at(0.45) < 0)
  assert.ok(Math.abs(left.at(0.45) + right.at(0.45)) < 1e-9)
})

test('a bigger turn is a bigger lag, up to the slip the swing is scaled to', () => {
  const small = turn(new ArmFollowController(), 0.3, 0.15, 1)
  const large = turn(new ArmFollowController(), 0.3, 0.45, 1)
  assert.ok(large.peak > small.peak * 2)
  const beyond = turn(new ArmFollowController(), 0.05, FULL_SLIP_RADIANS * 6, 1)
  assert.ok(beyond.peak <= 1)
})

test('a portrait with no sleeve layer gets no swing at all', () => {
  const swept = turn(new ArmFollowController(), 0.3, 0.45, 1, 60, 0)
  assert.equal(swept.peak, 0)
})

test('the asset rigid-arm allowance bounds the swing', () => {
  const swept = turn(new ArmFollowController(), 0.05, 1.2, 1, 60, 0.3)
  assert.ok(swept.peak <= 0.3 + 1e-9, `${swept.peak}`)
})

test('the lift is the swing, so it can never disagree with it', () => {
  const follow = new ArmFollowController()
  let checked = 0
  for (let index = 0; index <= 120; index += 1) {
    const yaw = index < 18 ? (index / 18) * 0.45 : 0.45
    const state = follow.step(yaw, 1 / 60, 1)
    // A pendulum rises whichever way it swings.
    assert.ok(state.lift >= 0)
    assert.ok(
      Math.abs(state.lift - state.swing * state.swing * ARM_SWING_LIFT) < 1e-12,
    )
    if (state.lift > 0) checked += 1
  }
  assert.ok(checked > 30)
})

test('30, 60 and 120 fps answer the same turn the same way', () => {
  // A step is the exact case: the two lag stages are integrated in closed
  // form, so the same elapsed time has to give the same lag whatever the
  // frame rate split it into.
  const lagAt = (fps: number, seconds: number): number => {
    const dt = 1 / fps
    const follow = new ArmFollowController()
    follow.step(0, 0, 1)
    let swing = 0
    for (let index = 1; index <= Math.round(seconds / dt); index += 1) {
      swing = follow.step(0.45, dt, 1).swing
    }
    return swing
  }
  for (const seconds of [0.1, 0.2, 0.5]) {
    const values = [30, 60, 120].map((fps) => lagAt(fps, seconds))
    for (const value of values) {
      assert.ok(Math.abs(value - values[1]) < 1e-9, `${values.join(', ')}`)
    }
  }
})

test('a long dropped frame cannot manufacture a swing', () => {
  const follow = new ArmFollowController()
  follow.step(0.45, 1 / 60, 1)
  for (let index = 0; index < 40; index += 1) follow.step(0.45, 1 / 60, 1)
  const stepped = follow.step(0.45, 1 / 60, 1).swing
  const dropped = new ArmFollowController()
  dropped.step(0.45, 1 / 60, 1)
  dropped.step(0.45, 40 / 60, 1)
  assert.ok(Math.abs(dropped.step(0.45, 1 / 60, 1).swing - stepped) < 1e-6)
})

test('a non-finite torso rotation or step is refused, not propagated', () => {
  const follow = new ArmFollowController()
  follow.step(0.2, 1 / 60, 1)
  assert.equal(Number.isFinite(follow.step(Number.NaN, 1 / 60, 1).swing), true)
  assert.equal(
    Number.isFinite(follow.step(0.2, Number.POSITIVE_INFINITY, 1).swing),
    true,
  )
  assert.equal(Number.isFinite(follow.step(0.2, 1 / 60, Number.NaN).swing), true)
})
