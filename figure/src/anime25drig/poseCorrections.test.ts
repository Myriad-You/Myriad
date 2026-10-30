import type { PoseCorrection } from './poseCorrections'
import assert from 'node:assert/strict'
import test from 'node:test'
import { IDENTITY_DRIVER } from './driver'
import {
  applyPoseCorrections,
  bindPoseCorrections,
  isPoseCorrections,
  poseCorrectionPatch,
  writePoseCorrectionWeights,
} from './poseCorrections'

const head = {
  centerX: 300,
  centerY: 250,
  radiusX: 160,
  radiusY: 200,
  radiusZ: 100,
}
const correction: PoseCorrection = {
  surface: 'head',
  at: { angleX: 0.8, angleY: -0.6 },
  patches: [{ x: 0, y: 0, radiusX: 0.6, radiusY: 0.4, dx: 0.05, dy: -0.04 }],
}
const rest = new Float32Array([300, 250, 320, 270, 650, 800])

function sample(angleX: number, angleY: number) {
  const bound = bindPoseCorrections([correction], 'head', rest, head)!
  writePoseCorrectionWeights(bound, { ...IDENTITY_DRIVER, angleX, angleY })
  const result = { x: 300, y: 250 }
  applyPoseCorrections(result, 0, bound)
  return result
}

test('combination residual preserves neutral, both single axes and opposite quadrants exactly', () => {
  for (const [x, y] of [
    [0, 0],
    [0.8, 0],
    [0, -0.6],
    [-0.8, -0.6],
    [0.8, 0.6],
  ]) {
    assert.deepEqual(sample(x, y), { x: 300, y: 250 })
  }
  assert.deepEqual(sample(0.8, -0.6), { x: 308, y: 242 })
  assert.deepEqual(
    sample(1, -1),
    sample(0.8, -0.6),
    'no extrapolation beyond the authored corner',
  )
})

test('local brushes leave distant vertices and other surfaces untouched', () => {
  const bound = bindPoseCorrections([correction], 'head', rest, head)!
  writePoseCorrectionWeights(bound, {
    ...IDENTITY_DRIVER,
    angleX: 0.8,
    angleY: -0.6,
  })
  const point = { x: rest[4], y: rest[5] }
  applyPoseCorrections(point, 2, bound)
  assert.deepEqual(point, { x: 650, y: 800 })
  assert.equal(bindPoseCorrections([correction], null, rest, head), undefined)
  assert.equal(
    bindPoseCorrections([correction], 'back-hair', rest, head),
    undefined,
  )
  assert.equal(bindPoseCorrections(undefined, 'head', rest, head), undefined)
})

test('closure combinations use continuous final eye aperture, not expression names', () => {
  const bound = bindPoseCorrections(
    [{ ...correction, at: { angleX: 0.8, eyeCloseL: 1 } }],
    'head',
    rest,
    head,
  )!
  const weights = []
  for (const eyeOpenL of [1, 0.75, 0.5, 0.25, 0]) {
    writePoseCorrectionWeights(bound, {
      ...IDENTITY_DRIVER,
      angleX: 0.8,
      eyeOpenL,
    })
    weights.push(bound[0].weight)
  }
  assert.deepEqual(weights, [0, 0.15625, 0.5, 0.84375, 1])
})

test('weights and spatial influence have no boundary slope jump', () => {
  const h = 1e-5
  assert.ok(Math.abs((sample(h, -0.6).x - sample(0, -0.6).x) / h) < 0.001)
  assert.ok(
    Math.abs((sample(0.8, -0.6).x - sample(0.8 - h, -0.6).x) / h) < 0.001,
  )
  const border = head.centerX + correction.patches[0].radiusX * head.radiusX
  const bound = bindPoseCorrections(
    [correction],
    'head',
    new Float32Array([border - 0.01, 250, border, 250]),
    head,
  )!
  assert.ok(Math.abs(bound[0].offsets[0] / 0.01) < 1e-6)
  assert.equal(bound[0].offsets[2], 0)
})

test('30/60/120 fps and interrupted/repeated poses have no history or additional delay', () => {
  for (const fps of [30, 60, 120]) {
    const bound = bindPoseCorrections([correction], 'head', rest, head)!
    for (let frame = 0; frame <= fps; frame++) {
      const angleX = Math.sin((frame / fps) * 2)
      const angleY = -Math.cos(frame / fps)
      writePoseCorrectionWeights(bound, { ...IDENTITY_DRIVER, angleX, angleY })
      const point = { x: 300, y: 250 }
      applyPoseCorrections(point, 0, bound)
      assert.deepEqual(point, sample(angleX, angleY))
    }
    writePoseCorrectionWeights(bound, IDENTITY_DRIVER)
    assert.equal(bound[0].weight, 0)
  }
})

test('authoring captures a residual and scales with the asset, not canvas pixels', () => {
  const patch = poseCorrectionPatch(
    head,
    { x: 300, y: 250 },
    { x: 350, y: 280 },
    { x: 358, y: 272 },
    { x: 96, y: 80 },
  )
  assert.deepEqual(patch, correction.patches[0])
  const scaled = {
    centerX: 600,
    centerY: 500,
    radiusX: 320,
    radiusY: 400,
    radiusZ: 200,
  }
  const bound = bindPoseCorrections(
    [correction],
    'head',
    new Float32Array([600, 500]),
    scaled,
  )!
  writePoseCorrectionWeights(bound, {
    ...IDENTITY_DRIVER,
    angleX: 0.8,
    angleY: -0.6,
  })
  const point = { x: 700, y: 560 }
  applyPoseCorrections(point, 0, bound)
  assert.deepEqual(point, { x: 716, y: 544 })
})

test('rejects malformed, unbounded, single-axis and duplicate-corner data', () => {
  assert.equal(isPoseCorrections([]), true)
  assert.equal(isPoseCorrections([correction]), true)
  for (const bad of [
    null,
    {},
    [null],
    Array.from({ length: 17 }).fill(correction),
    [{ ...correction, at: { angleX: 0.8 } }],
    [{ ...correction, at: { angleX: 0, angleY: 0.6 } }],
    [{ ...correction, at: { angleX: 0.6, eyeCloseL: -1 } }],
    [{ ...correction, at: { eyeCloseL: 1, mouthOpen: 1 } }],
    [{ ...correction, at: { angleX: 0.6, body: 1 } }],
    [{ ...correction, at: { angleX: Infinity, angleY: 0.6 } }],
    [{ ...correction, patches: [] }],
    [{ ...correction, patches: [null] }],
    [{ ...correction, patches: [{ ...correction.patches[0], radiusX: 0 }] }],
    [{ ...correction, patches: [{ ...correction.patches[0], dx: 0.3 }] }],
    [correction, { ...correction, at: { angleY: -1, angleX: 1 } }],
  ])
    assert.equal(isPoseCorrections(bad), false, JSON.stringify(bad))
})
