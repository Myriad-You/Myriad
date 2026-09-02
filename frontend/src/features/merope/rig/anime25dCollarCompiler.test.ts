import type { Anime25DRiggerAnchors } from '../anime25drig/playback'
import type { RasterLayer } from './anime25dImportTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { splitHighCollarOcclusion } from './anime25dCollarCompiler'

const anchors = {
  face: { x0: 20, y0: 0, x1: 60, y1: 20, cx: 40, cy: 10 },
  neckPivot: { x: 40, y: 45 },
  neckTop: 20,
  neckBottom: 69,
  bodyPivot: { x: 40, y: 80 },
  mouth: { x0: 34, y0: 12, x1: 46, y1: 17, cx: 40, cy: 14 },
  faceScale: 1,
} as Anime25DRiggerAnchors

test('keeps ordinary low clothing untouched', () => {
  const neck = raster('neck', 20, 20, 40, 50, () => [245, 205, 190, 255])
  const topwear = raster('topwear', 0, 55, 80, 45, () => [90, 80, 120, 255])
  const layers = [topwear, neck]
  assert.equal(splitHighCollarOcclusion(layers, anchors), layers)
})

test('splits a standalone high collar into rear, neck and deformable front topology', () => {
  const neck = raster('neck', 20, 20, 40, 50, () => [245, 205, 190, 255])
  const topwear = raster('topwear', 0, 0, 80, 100, (_x, y) =>
    y >= 20 && y <= 35 ? [42, 45, 62, 255] : [184, 176, 196, 255],
  )
  const output = splitHighCollarOcclusion([topwear, neck], anchors)
  const roles = output.map((layer) => layer.role)
  const rear = roles.indexOf('collar-back')
  const preservedNeck = roles.indexOf('neck')
  const front = roles.indexOf('collar-front')

  assert.ok(rear >= 0, 'darker upper-edge material becomes the rear collar')
  assert.ok(
    front >= 0,
    'the complementary visible material becomes the front collar',
  )
  assert.ok(rear < preservedNeck && preservedNeck < front)
  assert.equal(
    output[preservedNeck],
    neck,
    'neck pixels are never destructively cut',
  )
  assert.ok(
    output[rear].data.some((value, index) => index % 4 === 3 && value > 0),
  )
  assert.ok(
    output[front].data.some((value, index) => index % 4 === 3 && value > 0),
  )
})

function raster(
  role: 'neck' | 'topwear',
  left: number,
  top: number,
  width: number,
  height: number,
  color: (x: number, y: number) => readonly [number, number, number, number],
): RasterLayer {
  const data = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      data.set(color(left + x, top + y), (y * width + x) * 4)
    }
  }
  return {
    id: role,
    role,
    sourceName: role,
    order: role === 'topwear' ? 0 : 1,
    side: null,
    group: 'body',
    left,
    top,
    width,
    height,
    data,
  }
}
