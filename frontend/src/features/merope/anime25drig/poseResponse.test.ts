import assert from 'node:assert/strict'
import test from 'node:test'
import { IDENTITY_DRIVER } from './driver'
import { CONTINUOUS_POSE_KEYS, PoseResponseController } from './poseResponse'

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
