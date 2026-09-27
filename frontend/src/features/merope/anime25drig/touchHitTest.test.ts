import type { TouchMesh } from './touchHitTest'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  writeAnime25DLayerGlobalTransform,
  writeIdentityLayerTransform,
} from './layerTransform'
import { bodyLeanShare } from './poseScale'
import {
  hitTestTouchMesh,
  sampleTouchAlpha,

  touchPointInView,
} from './touchHitTest'

const frame = {
  bodyPivotX: 50,
  bodyPivotY: 100,
  bodyRotationCosine: 1,
  bodyRotationSine: 0,
}
function square(): TouchMesh {
  const layerTransform = new Float32Array(9)
  writeIdentityLayerTransform(layerTransform)
  return {
    positions: new Float32Array([0, 0, 100, 0, 0, 100, 100, 100]),
    atlasUvs: new Float32Array([0, 0, 1, 0, 0, 1, 1, 1]),
    indices: new Uint16Array([0, 1, 2, 1, 3, 2]),
    layerTransform,
  }
}

test('triangle hit returns interpolated atlas UVs, not bounding-box coordinates', () => {
  const mesh = square()
  mesh.positions[6] = 150
  const hit = hitTestTouchMesh(125, 100, mesh, frame)!
  assert.ok(Math.abs(hit.u - 5 / 6) < 1e-6)
  assert.equal(hit.v, 1)
  assert.equal(hitTestTouchMesh(125, 10, mesh, frame), null)
})

test('picking follows the actual global layer transform then the body rotation', () => {
  const mesh = square()
  writeAnime25DLayerGlobalTransform(
    {
      headFollow: 0.8,
      headRotationCosine: Math.cos(0.3),
      headRotationSine: Math.sin(0.3),
      neckPivotX: 50,
      neckPivotY: 90,
      faceScale: 1,
      angleX: 0.6,
      angleY: -0.2,
      depthOffset: 0.4,
      faceCenterY: 50,
      specialOffsetY: 5,
      breathOffset: 2,
    },
    mesh.layerTransform,
  )
  const body = {
    ...frame,
    bodyRotationCosine: Math.cos(-0.2),
    bodyRotationSine: Math.sin(-0.2),
  }
  const m = mesh.layerTransform
  const lx = m[0] * 20 + m[3] * 35 + m[6]
  const ly = m[1] * 20 + m[4] * 35 + m[7]
  const dx = lx - body.bodyPivotX
  const dy = ly - body.bodyPivotY
  const hit = hitTestTouchMesh(
    body.bodyPivotX + dx * body.bodyRotationCosine - dy * body.bodyRotationSine,
    body.bodyPivotY + dx * body.bodyRotationSine + dy * body.bodyRotationCosine,
    mesh,
    body,
  )!
  assert.ok(Math.abs(hit.u - 0.2) < 1e-6)
  assert.ok(Math.abs(hit.v - 0.35) < 1e-6)
})

test('degenerate and invalid geometry cannot manufacture hits', () => {
  const mesh = square()
  assert.equal(hitTestTouchMesh(NaN, 20, mesh, frame), null)
  mesh.positions.fill(0)
  assert.equal(hitTestTouchMesh(0, 0, mesh, frame), null)
  mesh.layerTransform.fill(0)
  assert.equal(hitTestTouchMesh(0, 0, mesh, frame), null)
})

test('same portrait point maps identically at panel/widget scales and offsets', () => {
  for (const scale of [0.25, 1, 2.5]) {
    const bounds = {
      left: 30,
      top: -70,
      width: 768 * scale,
      height: 1024 * scale,
    }
    assert.deepEqual(
      touchPointInView(30 + 192 * scale, -70 + 256 * scale, bounds, {
        width: 768,
        height: 1024,
      }),
      { x: 192, y: 256 },
    )
  }
  const bounds = { left: 0, top: 0, width: 100, height: 100 }
  const view = { width: 768, height: 1024 }
  assert.equal(touchPointInView(-1, 0, bounds, view), null)
  assert.equal(touchPointInView(100, 0, bounds, view), null)
  assert.equal(touchPointInView(0, 0, { ...bounds, width: 0 }, view), null)
})

test('alpha lookup rejects transparent texels and interpolates their borders', () => {
  const alpha = new Uint8Array([0, 255, 0, 255])
  assert.equal(sampleTouchAlpha(alpha, 2, 2, 0.25, 0.25), 0)
  assert.equal(sampleTouchAlpha(alpha, 2, 2, 0.75, 0.25), 1)
  assert.equal(sampleTouchAlpha(alpha, 2, 2, 0.5, 0.5), 0.5)
  assert.equal(sampleTouchAlpha(alpha, 2, 2, 1, 1), 1)
  assert.equal(sampleTouchAlpha(alpha, 2, 2, -1, 0), 0)
  assert.equal(sampleTouchAlpha(alpha, 0, 2, 0, 0), 0)
})

test('picking undoes the torso bend: the cut stays put and the shoulders take the whole lean', () => {
  const mesh = square()
  const lean = 0.07
  const bent = {
    bodyPivotX: 50,
    bodyPivotY: 100,
    bodyRotationCosine: Math.cos(lean),
    bodyRotationSine: Math.sin(lean),
    bodyBendHeight: 60,
  }
  // Where the vertex shader draws a mesh point, given the lean it takes at its height.
  const draw = (x: number, y: number) => {
    const angle = lean * bodyLeanShare(y, 100, 60)
    const dx = x - 50
    const dy = y - 100
    return [50 + dx * Math.cos(angle) - dy * Math.sin(angle), 100 + dx * Math.sin(angle) + dy * Math.cos(angle)]
  }
  for (const [x, y] of [[20, 99], [80, 70], [30, 30], [70, 10]]) {
    const [sx, sy] = draw(x, y)
    const hit = hitTestTouchMesh(sx, sy, mesh, bent)!
    assert.ok(Math.abs(hit.u - x / 100) < 1e-3 && Math.abs(hit.v - y / 100) < 1e-3, `${x},${y} -> ${hit.u},${hit.v}`)
  }
})
