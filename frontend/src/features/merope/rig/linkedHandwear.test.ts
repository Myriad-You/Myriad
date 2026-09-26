import type { RasterLayer } from './anime25dImportTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { anime25DShoulderSeeds, splitLinkedHandwear } from './linkedHandwear'

function raster(role: RasterLayer['role'], left: number, top: number, width: number, height: number, filled: (x: number, y: number) => boolean): RasterLayer {
  const data = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      if (!filled(x, y)) continue
      data.set([200, 180, 160, 255], (y * width + x) * 4)
    }
  }
  return { id: role, role, sourceName: role, order: 0, side: null, group: 'body', left, top, width, height, data }
}

function opaqueCount(layer: RasterLayer) {
  let count = 0
  for (let index = 3; index < layer.data.length; index += 4) { if (layer.data[index] > 0) count += 1
}
  return count
}

// A face at x 80–120 over a neck ending at y 100: shoulders near x 71 and 129.
const face = raster('face', 80, 20, 40, 50, () => true)
const neck = raster('neck', 92, 60, 16, 40, () => true)

test('two arms joined at the hands split at the hands, one arm per shoulder', () => {
  // Two hanging arms whose hands meet in a bar across the body.
  const arms = raster('handwear', 50, 100, 100, 100, (x, y) => x < 30 || x >= 70 || y >= 85)
  const split = splitLinkedHandwear(arms, anime25DShoulderSeeds([face, neck])!, 100)!
  const at = (layer: RasterLayer, x: number, y: number) => layer.data[((y - 100) * 100 + (x - 50)) * 4 + 3]
  assert.equal(at(split.left, 55, 110), 255)
  assert.equal(at(split.right, 55, 110), 0)
  assert.equal(at(split.right, 145, 110), 255)
  assert.equal(at(split.left, 145, 110), 0)
  // The seam falls in the middle of the joining bar.
  assert.equal(at(split.left, 90, 195), 255)
  assert.equal(at(split.right, 110, 195), 255)
  // Nothing is lost or drawn twice.
  assert.equal(opaqueCount(split.left) + opaqueCount(split.right), opaqueCount(arms))
})

test('one arm on one side is not two arms', () => {
  const single = raster('handwear', 40, 100, 40, 100, () => true)
  assert.equal(splitLinkedHandwear(single, anime25DShoulderSeeds([face, neck])!, 100), null)
})

test('no face, no shoulders to split from', () => {
  assert.equal(anime25DShoulderSeeds([neck]), null)
})
