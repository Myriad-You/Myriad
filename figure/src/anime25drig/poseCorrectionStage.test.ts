import type { PoseCorrectionStageLayer } from './poseCorrectionStage'
import assert from 'node:assert/strict'
import test from 'node:test'
import { pickPoseCorrectionPoint, projectPoseCorrectionPatch, viewDeltaToPoseCorrection } from './poseCorrectionStage'

const head = { centerX: 100, centerY: 100, radiusX: 50, radiusY: 40, radiusZ: 40 }
const still = { bodyPivotX: 0, bodyPivotY: 0, bodyRotationCosine: 1, bodyRotationSine: 0 }
const identity = new Float32Array([1, 0, 0, 0, 1, 0, 0, 0, 1])
const AT = { angleX: 0.5, angleY: 0.3 }
const HEAD = { surface: 'head' as const, at: AT }
const AT_POSE = { angleX: 0.5, angleY: 0.3, eyeOpenL: 1, eyeOpenR: 1, mouthOpen: 0 }

/** A square from (50,50) to (150,150): two triangles, uvs across the atlas. */
function square(surface: PoseCorrectionStageLayer['surface'], shift = { x: 0, y: 0 }, transform = identity): PoseCorrectionStageLayer {
  const rest = new Float32Array([50, 50, 150, 50, 150, 150, 50, 150])
  const deformed = rest.map((v, i) => v + (i % 2 ? shift.y : shift.x))
  return {
    rest,
    deformed,
    atlasUvs: new Float32Array([0, 0, 1, 0, 1, 1, 0, 1]),
    indices: new Uint16Array([0, 1, 2, 0, 2, 3]),
    layerTransform: transform,
    frameOpacity: 1,
    surface,
  }
}

test('a click lands on the rest point drawn there, in head radii', () => {
  const pick = pickPoseCorrectionPoint(125, 80, [square('head')], still, head, null)
  assert.deepEqual(pick, { surface: 'head', x: 0.5, y: -0.5 })
  // The mesh is drawn shifted: the same screen point is a different rest point.
  const shifted = pickPoseCorrectionPoint(125, 80, [square('head', { x: 10, y: 0 })], still, head, null)
  assert.equal(shifted?.x, 0.3)
  assert.equal(pickPoseCorrectionPoint(10, 10, [square('head')], still, head, null), null)
  assert.equal(pickPoseCorrectionPoint(125, 80, [square(null)], still, head, null), null)
})

test('the topmost correctable layer wins, and a transparent pixel is not a surface', () => {
  const layers = [square('head'), square('front-hair')]
  assert.equal(pickPoseCorrectionPoint(125, 80, layers, still, head, null)?.surface, 'front-hair')
  const clear = { alpha: new Uint8Array(4), width: 2, height: 2 }
  assert.equal(pickPoseCorrectionPoint(125, 80, [square('head')], still, head, clear), null)
})

test('a patch is shown at its spot without it, and where it pushes the spot', () => {
  const still0 = { x: 0.5, y: -0.5, radiusX: 0.3, radiusY: 0.3, dx: 0, dy: 0 }
  const at = projectPoseCorrectionPatch(HEAD, still0, [square('head', { x: 10, y: -4 })], still, head, AT_POSE)
  assert.ok(at)
  assert.ok(Math.abs(at.origin.x - 135) < 1e-4 && Math.abs(at.origin.y - 76) < 1e-4)
  assert.deepEqual(at.pushed, at.origin)
  assert.equal(at.unitX, 50)
  assert.equal(at.unitY, 40)
  // The live mesh already carries this patch's push at its vertices; the
  // origin takes it back out, and the pushed point is the full push.
  const pushing = { x: 0, y: 0, radiusX: 2, radiusY: 2, dx: 0.1, dy: 0 }
  const layer = square('head')
  for (let i = 0; i < layer.rest.length; i += 2) {
    const r = Math.hypot((layer.rest[i] - 100) / 50 / 2, (layer.rest[i + 1] - 100) / 40 / 2)
    layer.deformed[i] += 0.1 * 50 * (1 - r) ** 4 * (1 + 4 * r)
  }
  const pushed = projectPoseCorrectionPatch(HEAD, pushing, [layer], still, head, AT_POSE)!
  assert.ok(Math.abs(pushed.origin.x - 100) < 1e-4)
  assert.ok(Math.abs(pushed.pushed.x - 105) < 1e-4)
  assert.equal(pushed.weight, 1)
  // A pose half way to the corner holds it at half weight and shows half the push.
  const half = { ...layer, deformed: layer.rest.map((v, i) => v + (i % 2 ? 0 : (layer.deformed[i] - v) / 2)) }
  const halfway = projectPoseCorrectionPatch(HEAD, pushing, [half], still, head, { ...AT_POSE, angleX: 0.25 })!
  assert.ok(Math.abs(halfway.origin.x - 100) < 1e-4)
  assert.ok(Math.abs(halfway.pushed.x - 102.5) < 1e-4)
  // Off the mesh, it follows its nearest vertex.
  const off = projectPoseCorrectionPatch(HEAD, { ...still0, x: 2, y: 2 }, [square('head', { x: 10, y: 0 })], still, head, AT_POSE)
  assert.ok(off && Math.abs(off.origin.x - 210) < 1e-4 && Math.abs(off.origin.y - 180) < 1e-4)
  assert.equal(projectPoseCorrectionPatch({ surface: 'back-hair', at: AT }, still0, [square('head')], still, head, AT_POSE), null)
})

test('a drag on screen becomes a displacement in head radii, through the layer scale', () => {
  const doubled = new Float32Array([2, 0, 0, 0, 2, 0, 0, 0, 1])
  const delta = viewDeltaToPoseCorrection(20, -16, [square('head', undefined, doubled)], 'head', still, head)
  assert.ok(Math.abs(delta.dx - 0.2) < 1e-6)
  assert.ok(Math.abs(delta.dy + 0.2) < 1e-6)
  const plain = viewDeltaToPoseCorrection(25, 20, [square('head')], 'head', still, head)
  assert.ok(Math.abs(plain.dx - 0.5) < 1e-6 && Math.abs(plain.dy - 0.5) < 1e-6)
})
