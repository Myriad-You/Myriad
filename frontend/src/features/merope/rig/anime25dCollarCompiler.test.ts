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

test('does not promote a duplicated shaded neck into clothing', () => {
  const skin = (
    _x: number,
    y: number,
  ): readonly [number, number, number, number] =>
    y < 36 ? [203, 163, 153, 255] : [245, 205, 190, 255]
  for (const noise of [0, 3, -3]) {
    const neck = raster('neck', 20, 20, 40, 50, skin)
    const topwear = raster('topwear', 0, 0, 80, 100, (x, y) => {
      const [r, g, b, a] = skin(x, y)
      return [r + noise, g + noise, b + noise, a]
    })
    const layers = [topwear, neck]
    const before = topwear.data.slice()
    assert.equal(splitHighCollarOcclusion(layers, anchors), layers)
    assert.deepEqual(topwear.data, before)
  }
})

test('a chain or narrow ornament over copied skin is not high-collar evidence', () => {
  const skin = [245, 205, 190, 255] as const
  const neck = raster('neck', 20, 20, 40, 50, () => skin)
  const topwear = raster('topwear', 0, 0, 80, 100, (x) =>
    Math.abs(x - 40) < 4 ? [90, 60, 150, 255] : skin,
  )
  const layers = [topwear, neck]
  assert.equal(splitHighCollarOcclusion(layers, anchors), layers)
})

test('uniform pale and dark real collars retain geometric front/rear partitioning', () => {
  for (const color of [
    [242, 240, 248, 255],
    [35, 30, 45, 255],
  ] as const) {
    const neck = raster('neck', 20, 20, 40, 50, () => [245, 205, 190, 255])
    const topwear = raster('topwear', 0, 0, 80, 100, () => color)
    const before = topwear.data.slice()
    const output = splitHighCollarOcclusion([topwear, neck], anchors)
    assert.deepEqual(
      output.map((l) => l.role),
      ['topwear', 'collar-back', 'neck', 'collar-front'],
    )
    assert.equal(output[2], neck)
    assert.deepEqual(topwear.data, before, 'original artwork is immutable')
    // Every garment pixel is allocated, rather than erased or duplicated.
    for (let y = 0; y < 100; y++) {
      for (let x = 0; x < 80; x++) {
        const alpha = output
          .filter((l) => l !== neck)
          .reduce((sum, l) => sum + alphaAt(l, x, y), 0)
        assert.ok(Math.abs(alpha - 255) <= 1)
      }
    }
  }
})

test('trusted original pixels preserve visible front detail and rear ordering', () => {
  const neck = raster('neck', 20, 20, 40, 50, () => [245, 205, 190, 255])
  const topwear = raster('topwear', 0, 0, 80, 100, () => [180, 175, 205, 255])
  const face = {
    ...raster('neck', 20, 0, 40, 20, () => [245, 205, 190, 255]),
    id: 'face',
    role: 'face' as const,
  }
  const reference = raster('topwear', 0, 0, 80, 100, (x, y) =>
    x >= 20 && x < 60 && y < 44 ? [245, 205, 190, 255] : [180, 175, 205, 255],
  )
  const output = splitHighCollarOcclusion(
    [topwear, neck, face],
    anchors,
    reference,
  )
  const rear = output.find((l) => l.role === 'collar-back')!
  const front = output.find((l) => l.role === 'collar-front')!
  assert.ok(rear && front)
  assert.equal(alphaAt(rear, 40, 28), 255)
  assert.equal(alphaAt(front, 40, 60), 255)
  assert.equal(alphaAt(front, 40, 28), 0)
  assert.ok(output.indexOf(rear) < output.indexOf(neck))
  assert.ok(output.indexOf(neck) < output.indexOf(front))
})

test('finds the one evidenced neckline beyond unrelated garment fragments', () => {
  const neck = raster('neck', 20, 20, 40, 50, () => [245, 205, 190, 255])
  const low = {
    ...raster('topwear', 0, 75, 80, 25, () => [40, 30, 60, 255]),
    id: 'topwear-1',
  }
  const high = {
    ...raster('topwear', 0, 0, 80, 100, () => [180, 175, 205, 255]),
    id: 'topwear-2',
  }
  const ornament = {
    ...raster('neck', 34, 55, 12, 30, () => [120, 30, 160, 255]),
    id: 'neckwear',
    role: 'neckwear' as const,
  }
  const output = splitHighCollarOcclusion([low, neck, high, ornament], anchors)
  assert.ok(output.some((l) => l.role === 'collar-front'))
  assert.ok(output.includes(low) && output.includes(ornament))
  assert.equal(
    splitHighCollarOcclusion(output, anchors),
    output,
    'already compiled topology is stable',
  )
})

test('does not guess between multiple overlapping high garments or recut authored collars', () => {
  const neck = raster('neck', 20, 20, 40, 50, () => [245, 205, 190, 255])
  const coat = raster('topwear', 0, 0, 80, 100, () => [45, 40, 55, 255])
  const shirt = { ...coat, id: 'shirt', data: coat.data.slice() }
  const ambiguous = [neck, coat, shirt]
  assert.equal(splitHighCollarOcclusion(ambiguous, anchors), ambiguous)
  const authored = [neck, coat, { ...shirt, role: 'collar-front' as const }]
  assert.equal(splitHighCollarOcclusion(authored, anchors), authored)
})

function alphaAt(layer: RasterLayer, x: number, y: number): number {
  x -= layer.left
  y -= layer.top
  return x < 0 || y < 0 || x >= layer.width || y >= layer.height
    ? 0
    : layer.data[(y * layer.width + x) * 4 + 3]
}

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
