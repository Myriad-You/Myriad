import type { Anime25DDriver } from './driver'
import type { FollowThroughKey } from './followThrough'
import assert from 'node:assert/strict'
import test from 'node:test'
import { IDENTITY_DRIVER } from './driver'
import { FOLLOW_THROUGH, FollowThroughController } from './followThrough'
import { MinimumJerkMotion } from './minimumJerk'

const DT = 1 / 60
const OPEN = { angleX: 1, angleY: 1, angleZ: 1, body: 1 }

/** A brisk head turn to `to`, then a hold; returns the shown angleX per frame. */
function turn(to: number, duration: number, bounds = OPEN, phys = true) {
  const motion = new MinimumJerkMotion()
  const controller = new FollowThroughController()
  const primary: Anime25DDriver = { ...IDENTITY_DRIVER, phys }
  const shown: Anime25DDriver = { ...IDENTITY_DRIVER }
  motion.retarget(0, to, duration)
  const trace: number[] = []
  const primaries: number[] = []
  for (let t = DT; t < 3; t += DT) {
    primary.angleX = motion.sample(t)
    const acceleration = (key: FollowThroughKey) => (key === 'angleX' ? motion.acceleration : 0)
    controller.step(primary, acceleration, DT, bounds, shown)
    trace.push(shown.angleX)
    primaries.push(primary.angleX)
  }
  return Object.assign(trace, { primaries })
}

test('a brisk turn carries past its mark, springs back and settles on it', () => {
  const trace = turn(0.6, 0.4)
  const peak = Math.max(...trace)
  assert.ok(peak > 0.6 * 1.04, `peak ${peak}`)
  // It comes back through the mark: a spring, not a slow drift.
  const afterPeak = trace.slice(trace.indexOf(peak))
  assert.ok(Math.min(...afterPeak) < 0.6, `${Math.min(...afterPeak)}`)
  assert.ok(Math.abs(trace.at(-1)! - 0.6) < 1e-3, `${trace.at(-1)}`)
})

test('a held pose is shown exactly as authored', () => {
  const controller = new FollowThroughController()
  const primary: Anime25DDriver = { ...IDENTITY_DRIVER, angleX: 0.4, angleY: -0.3, angleZ: 0.2, body: -0.5, phys: true }
  const shown: Anime25DDriver = { ...IDENTITY_DRIVER }
  for (let i = 0; i < 120; i++) controller.step(primary, () => 0, DT, OPEN, shown)
  assert.deepEqual(shown, primary)
})

test('a snap overshoots no further than the channel limit', () => {
  const trace = turn(0.5, 0.05)
  assert.ok(Math.max(...trace) - 0.5 <= FOLLOW_THROUGH.angleX.limit + 1e-9)
  assert.ok(Math.max(...trace) - 0.5 > FOLLOW_THROUGH.angleX.limit * 0.5)
})

test('the overshoot stays inside the motion envelope', () => {
  const trace = turn(0.75, 0.3, { ...OPEN, angleX: 0.8 })
  assert.ok(Math.max(...trace) <= 0.8 + 1e-9)
})

test('physics off shows the primary pose untouched', () => {
  const trace = turn(0.6, 0.4, OPEN, false)
  assert.deepEqual([...trace], trace.primaries)
})
