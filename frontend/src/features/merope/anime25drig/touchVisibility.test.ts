import type { TouchPaintLayer } from './touchVisibility'
import assert from 'node:assert/strict'
import test from 'node:test'
import { writeIdentityLayerTransform } from './layerTransform'
import { hitTestVisibleTouch, touchRegionForRole } from './touchVisibility'

const frame = { viewWidth: 100, viewHeight: 100, bodyPivotX: 50, bodyPivotY: 100,
  bodyRotationCosine: 1, bodyRotationSine: 0, time: 0, eyeCry: 0 }
const atlas = { alpha: new Uint8Array([255]), width: 1, height: 1 }
function layer(role: string): TouchPaintLayer {
  const transform = new Float32Array(9)
  writeIdentityLayerTransform(transform)
  return {
    paint: { source: { name: role, role, depth: 1, group: 'head', phys: null,
      fade: null, side: null, x: 0, y: 0, w: 100, h: 100,
      atlas: { x: 0, y: 0, w: 1, h: 1 }, strands: [] },
    renderKind: 'ordinary', frameOpacity: 1, retainWhenHidden: false, cryDirection: 0 },
    mesh: { positions: new Float32Array([0, 0, 100, 0, 0, 100, 100, 100]),
      atlasUvs: new Float32Array([0, 0, 1, 0, 0, 1, 1, 1]),
      indices: new Uint16Array([0, 1, 2, 1, 3, 2]), layerTransform: transform },
  }
}

test('paint order preserves accessories; opaque unknown art blocks anatomy behind it', () => {
  const face = layer('face')
  const accessory = layer('neckwear')
  assert.equal(hitTestVisibleTouch(50, 50, [face, accessory], atlas, frame)?.region, 'accessory')
  const unknown = layer('necklace-shadow')
  assert.equal(hitTestVisibleTouch(50, 50, [face, unknown], atlas, frame)?.region, null)
  assert.equal(touchRegionForRole('front-hair-shadow'), null)
})

test('hidden and transparent drawings do not intercept contact', () => {
  const face = layer('face')
  const hair = layer('front-hair')
  hair.paint.frameOpacity = 0
  hair.paint.retainWhenHidden = true
  assert.equal(hitTestVisibleTouch(50, 50, [face, hair], atlas, frame)?.region, 'face')
  assert.equal(hitTestVisibleTouch(50, 50, [face], { ...atlas, alpha: new Uint8Array([0]) }, frame), null)
})

test('iris picking is bounded by its own eye stencil even during expression fades', () => {
  const body = layer('topwear')
  const white = layer('eyewhite')
  white.paint.renderKind = 'eyewhite'
  white.paint.source.side = 'L'
  white.paint.frameOpacity = 0
  white.paint.retainWhenHidden = true
  const iris = layer('iris')
  iris.paint.renderKind = 'iris'
  iris.paint.source.side = 'R'
  assert.equal(hitTestVisibleTouch(50, 50, [body, white, iris], atlas, frame)?.region, 'body')
  iris.paint.source.side = 'L'
  assert.equal(hitTestVisibleTouch(50, 50, [body, white, iris], atlas, frame)?.region, 'face')
  white.mesh!.layerTransform[6] = 200
  assert.equal(hitTestVisibleTouch(50, 50, [body, white, iris], atlas, frame)?.region, 'body')
})

test('neck surface fade does not hide a necklace underneath its transparent tail', () => {
  const accessory = layer('neckwear')
  const neck = layer('neck')
  neck.paint.renderKind = 'neck'
  neck.paint.neckSurfaceFade = { start: 0.4, end: 0.6 }
  assert.equal(hitTestVisibleTouch(50, 20, [accessory, neck], atlas, frame)?.region, 'body')
  assert.equal(hitTestVisibleTouch(50, 80, [accessory, neck], atlas, frame)?.region, 'accessory')
  neck.paint.neckSurfaceFade.contour = { left: 0, right: 1, bands: new Float32Array([0.1, 0.2, 0.8, 0.9]) }
  assert.equal(hitTestVisibleTouch(0, 50, [accessory, neck], atlas, frame)?.region, 'accessory')
  assert.equal(hitTestVisibleTouch(100, 50, [accessory, neck], atlas, frame)?.region, 'body')
})

test('replacement neck mesh, not its source rectangle, defines the high-collar hit', () => {
  const collar = layer('collar-front')
  const neck = layer('neck')
  neck.mesh!.positions = new Float32Array([40, 0, 60, 0, 40, 30, 60, 30])
  assert.equal(hitTestVisibleTouch(50, 15, [collar, neck], atlas, frame)?.layerIndex, 1)
  assert.equal(hitTestVisibleTouch(50, 70, [collar, neck], atlas, frame)?.layerIndex, 0)
})

test('animated tears are not a touchable floating extension of the face', () => {
  const tear = layer('iris')
  tear.paint.cryDirection = 1
  assert.equal(hitTestVisibleTouch(50, 50, [tear], atlas, frame), null)
})
