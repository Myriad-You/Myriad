import assert from 'node:assert/strict'
import test from 'node:test'
import { IDENTITY_DRIVER } from './driver'
import {
  CONTINUOUS_POSE_KEYS,
  MAX_RESPONSE_SCALE,
  MIN_RESPONSE_SCALE,
  PoseResponseController,
  poseResponseScale,
  resolvePoseResponseScale,
} from './poseResponse'

test('replacing an action starts at the actual pose and keeps its incoming velocity', () => {
  const response = new PoseResponseController()
  const current = { ...IDENTITY_DRIVER, angleX: 0.35, body: -0.2 }
  const target = { ...current, angleX: 0.9, body: 0.7 }
  response.step(current, target, 0.025)
  const epsilon = 1e-7
  const before = current.angleX
  response.step(current, target, epsilon)
  const incoming = (current.angleX - before) / epsilon
  const atHandoff = { ...current }
  const replacement = { ...target, angleX: -0.8, body: -0.7 }
  response.step(current, replacement, 0)
  assert.deepEqual(current, atHandoff)
  response.step(current, replacement, epsilon)
  const outgoing = (current.angleX - atHandoff.angleX) / epsilon
  assert.ok(incoming > 1)
  assert.ok(Math.abs(outgoing - incoming) < 0.001, `${incoming} -> ${outgoing}`)
  for (let i = 0; i < 30; i += 1) response.step(current, replacement, 1 / 60)
  assert.ok(Math.abs(current.angleX - replacement.angleX) < 0.001)
})

test('responds in the next frame and reaches the pose faster than the replaced filter', () => {
  const response = new PoseResponseController()
  const current = { ...IDENTITY_DRIVER }
  const target = { ...current, angleX: 1, eyeX: 1, body: 1 }
  response.step(current, target, 1 / 60)
  assert.ok(current.angleX > 0.07)
  assert.ok(current.eyeX > current.angleX)
  response.step(current, target, 0.12 - 1 / 60)
  assert.ok(current.angleX > 0.96)
  assert.ok(current.body > 0.89)
})

test('a goal reversal also carries acceleration instead of introducing impulsive jerk', () => {
  const response = new PoseResponseController()
  const current = { ...IDENTITY_DRIVER }
  const target = { ...current, angleX: 0.9 }
  response.step(current, target, 0.025)
  const epsilon = 1e-6
  const x0 = current.angleX
  response.step(current, target, epsilon)
  const x1 = current.angleX
  response.step(current, target, epsilon)
  const x2 = current.angleX
  const incoming = (x2 - 2 * x1 + x0) / epsilon ** 2
  const replacement = { ...target, angleX: -0.8 }
  response.step(current, replacement, epsilon)
  const x3 = current.angleX
  response.step(current, replacement, epsilon)
  const x4 = current.angleX
  const outgoing = (x4 - 2 * x3 + x2) / epsilon ** 2
  assert.ok(Math.abs(outgoing - incoming) < 0.5, `${incoming} -> ${outgoing}`)
})

test('all motion channels agree at 30/60/120Hz and remain bounded through repeated reversals', () => {
  const run = (fps: number) => {
    const response = new PoseResponseController()
    const current = { ...IDENTITY_DRIVER }
    const snapshots = []
    for (let phase = 0; phase < 20; phase += 1) {
      const target = { ...IDENTITY_DRIVER }
      for (const key of CONTINUOUS_POSE_KEYS)
        target[key] = phase % 2 === 0 ? 1 : -1
      for (let frame = 0; frame < fps / 10; frame += 1) {
        response.step(current, target, 1 / fps)
        for (const key of CONTINUOUS_POSE_KEYS)
          assert.ok(Math.abs(current[key]) <= 1 + 1e-12)
      }
      snapshots.push({ ...current })
    }
    return snapshots
  }
  const a = run(30)
  for (const fps of [60, 120]) {
    const b = run(fps)
    a.forEach((pose, i) => {
      for (const key of CONTINUOUS_POSE_KEYS)
        assert.ok(Math.abs(pose[key] - b[i][key]) < 1e-10)
    })
  }
})

const NEUTRAL_QUALITY = {
  extent: 1,
  tempo: 1,
  power: 1,
  fluidity: 0.8,
  directness: 0.72,
  rebound: 0.35,
  asymmetry: 0.2,
  density: 0.8,
}

// The two profiles `compilePerformanceBehaviorPlan` authors for its cues.
const FORCEFUL = {
  ...NEUTRAL_QUALITY,
  tempo: 1.2,
  power: 1.2,
  fluidity: 0.58,
  directness: 0.9,
}
const GENTLE = {
  ...NEUTRAL_QUALITY,
  tempo: 0.9,
  power: 0.8,
  fluidity: 0.82,
  directness: 0.72,
}

test('a delivery neither snaps nor drifts outside the rig bandwidth', () => {
  assert.equal(poseResponseScale(NEUTRAL_QUALITY), 1)
  for (const quality of [FORCEFUL, GENTLE]) {
    const scale = poseResponseScale(quality)
    assert.ok(scale >= MIN_RESPONSE_SCALE && scale <= MAX_RESPONSE_SCALE)
  }
  const extreme = poseResponseScale({
    ...NEUTRAL_QUALITY,
    tempo: 99,
    power: 99,
    directness: 99,
    fluidity: -99,
  })
  assert.equal(extreme, MAX_RESPONSE_SCALE)
})

test('manner reaches the pose, not just its size', () => {
  assert.ok(poseResponseScale(FORCEFUL) > poseResponseScale(GENTLE))

  const travel = (quality: typeof FORCEFUL): number => {
    const response = new PoseResponseController()
    const current = { ...IDENTITY_DRIVER }
    const target = { ...IDENTITY_DRIVER, angleX: 1 }
    const scale = poseResponseScale(quality)
    for (let step = 0; step < 4; step += 1) {
      response.step(current, target, 1 / 60, scale)
    }
    return current.angleX
  }
  // Same goal, same elapsed time. Before this, a forceful `emphasize` and a
  // soft `listen` nod arrived at exactly the same place.
  assert.ok(
    travel(FORCEFUL) > travel(GENTLE) * 1.1,
    `${travel(FORCEFUL)} vs ${travel(GENTLE)}`,
  )
})

test('an unclaimed pose keeps the rig own rate', () => {
  assert.equal(resolvePoseResponseScale([]), 1)
  assert.equal(
    resolvePoseResponseScale([{ weight: 0.8, quality: null }]),
    1,
  )
  // Idle drift has no authored manner, so a beat that carries a tenth of the
  // pose may only move the bandwidth a tenth of the way.
  const faint = resolvePoseResponseScale([{ weight: 0.1, quality: FORCEFUL }])
  const full = resolvePoseResponseScale([{ weight: 1, quality: FORCEFUL }])
  assert.ok(faint > 1 && faint < full)
  assert.ok(Math.abs(faint - 1 - (full - 1) * 0.1) < 1e-9)
})

test('two live sources are averaged by what each carries', () => {
  const blended = resolvePoseResponseScale([
    { weight: 0.9, quality: FORCEFUL },
    { weight: 0.1, quality: GENTLE },
  ])
  assert.ok(blended > resolvePoseResponseScale([{ weight: 1, quality: GENTLE }]))
  assert.ok(
    blended < resolvePoseResponseScale([{ weight: 1, quality: FORCEFUL }]),
  )
})
