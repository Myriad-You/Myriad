import type { Anime25DPlaybackLayer } from './types'
import type { CroppedLayerPixels } from './webglRuntime'
import assert from 'node:assert/strict'
import test from 'node:test'
import { deriveCrownOcclusionBand, unionDrawing } from './crownOcclusion'

const drawing = (role: Anime25DPlaybackLayer['role']) => ({ role, x: 0, y: 0, w: 100, h: 100 })
function raster(alpha = 255): CroppedLayerPixels {
  return { width: 100, height: 100, pixels: new Uint8ClampedArray(100 * 100 * 4).fill(alpha) }
}
const face = drawing('face'); const front = drawing('front-hair'); const back = drawing('back-hair')

test('continuous upper scalp evidence produces a local band ending before the eyes without editing art', () => {
  const image = raster(); const before = image.pixels.slice()
  const band = deriveCrownOcclusionBand(face, front, back, 60, image, image, image)
  assert.deepEqual(band, { start: 0.39, end: 0.51 })
  const tallerBack = { ...back, y: -20, h: 200 }
  assert.deepEqual(deriveCrownOcclusionBand(face, front, tallerBack, 60, image, image, image), { start: 0.295, end: 0.355 })
  assert.deepEqual(image.pixels, before)
})

test('missing, translucent or sparse art never invents a scalp cap', () => {
  const solid = raster()
  for (const images of [[null, solid, solid], [solid, null, solid], [solid, solid, null],
    [raster(0), solid, solid], [solid, raster(180), solid], [solid, solid, raster(220)]] as const) {
    assert.equal(deriveCrownOcclusionBand(face, front, back, 60, ...images), null)
  }
  const parted = raster()
  for (let y = 0; y < 60; y++) { for (let x = 40; x < 60; x++) parted.pixels[(y * 100 + x) * 4 + 3] = 0
}
  assert.equal(deriveCrownOcclusionBand(face, front, back, 60, solid, parted, solid), null)
})

test('incompatible geometry and missing eye anchors do not reorder hair', () => {
  const solid = raster()
  for (const eyeTop of [NaN, Infinity, 20, 90]) {
    assert.equal(deriveCrownOcclusionBand(face, front, back, eyeTop, solid, solid, solid), null)
  }
  for (const invalidBack of [{ ...back, y: 1 }, { ...back, w: 50 }, drawing('front-hair')]) {
    assert.equal(deriveCrownOcclusionBand(face, front, invalidBack, 60, solid, solid, solid), null)
  }
  assert.equal(deriveCrownOcclusionBand(face, { ...front, y: 10 }, back, 60, solid, solid, solid), null)
})

test('a narrow early forehead part limits the band even when average fringe coverage passes', () => {
  const solid = raster()
  const parted = raster()
  for (let y = 30; y < 60; y++) {
    for (let x = 60; x < 69; x++) parted.pixels[(y * 100 + x) * 4 + 3] = 0
  }
  const band = deriveCrownOcclusionBand(face, front, back, 60, solid, parted, solid)
  assert.ok(band, 'keep the supported cap above the part')
  assert.ok(band.end <= 0.3, `do not repaint the exposed forehead: ${band.end}`)
  assert.ok(band.start < band.end)
})

test('a normal lower fringe opening keeps the existing cap band', () => {
  const solid = raster()
  const fringe = raster()
  for (let y = 40; y < 60; y++) {
    for (let x = 60; x < 69; x++) fringe.pixels[(y * 100 + x) * 4 + 3] = 0
  }
  assert.deepEqual(deriveCrownOcclusionBand(face, front, back, 60, solid, fringe, solid), { start: 0.39, end: 0.51 })
})

test('front hair cut into locks covers the scalp together', () => {
  const solid = { pixels: new Uint8ClampedArray(10 * 10 * 4).fill(255), width: 10, height: 10 }
  const left = { drawing: { role: 'front-hair', x: 0, y: 0, w: 50, h: 100 }, pixels: solid }
  const right = { drawing: { role: 'front-hair', x: 50, y: 0, w: 50, h: 100 }, pixels: solid }
  const joint = unionDrawing([left, right])!
  assert.deepEqual(joint.drawing, { role: 'front-hair', x: 0, y: 0, w: 100, h: 100 })
  // Every sample of the joint box is covered by one lock or the other.
  assert.ok(joint.pixels!.pixels.filter((_, i) => i % 4 === 3).every((a) => a === 255))
  // One layer stands as it is.
  assert.equal(unionDrawing([left]), left)
  assert.equal(unionDrawing([{ ...left, pixels: null }]), null)
})
