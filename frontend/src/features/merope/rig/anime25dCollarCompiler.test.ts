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

test('a long dress painted between the neck and the jacket stays under the jacket', () => {
  const neck = raster('neck', 20, 20, 40, 50, () => [245, 205, 190, 255])
  const topwear = raster('topwear', 0, 0, 80, 100, (_x, y) =>
    y >= 20 && y <= 35 ? [42, 45, 62, 255] : [184, 176, 196, 255],
  )
  const dress = { ...raster('topwear', 5, 40, 70, 60, () => [120, 118, 110, 255]), id: 'dress', role: 'bottomwear' as const }
  const output = splitHighCollarOcclusion([neck, dress, topwear], anchors)
  const roles = output.map((layer) => layer.role)
  assert.ok(roles.includes('collar-back'), roles.join(','))
  assert.ok(roles.indexOf('bottomwear') < roles.indexOf('topwear'), roles.join(','))
  assert.ok(roles.indexOf('topwear') < roles.indexOf('neck'))
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
    // Every garment pixel is allocated between the top and the back collar,
    // never erased; what goes in front is also kept in the top, under it.
    const [rest, back, , front] = output
    for (let y = 0; y < 100; y++) {
      for (let x = 0; x < 80; x++) {
        assert.ok(Math.abs(alphaAt(rest, x, y) + alphaAt(back, x, y) - 255) <= 1)
        assert.ok(alphaAt(front, x, y) <= alphaAt(rest, x, y) + 1)
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

test('the high collar beside the neck joins the front collar, so a neck sliding with a turn goes behind it', () => {
  const neck = raster('neck', 20, 20, 40, 50, () => [245, 205, 190, 255])
  const topwear = raster('topwear', 0, 0, 80, 100, (_x, y) =>
    y >= 20 && y <= 35 ? [42, 45, 62, 255] : [184, 176, 196, 255],
  )
  const output = splitHighCollarOcclusion([topwear, neck], anchors)
  const front = output.find((layer) => layer.role === 'collar-front')!
  const rest = output.find((layer) => layer.role === 'topwear')!
  const alpha = (layer: RasterLayer, x: number, y: number) => {
    const lx = x - layer.left
    const ly = y - layer.top
    if (lx < 0 || ly < 0 || lx >= layer.width || ly >= layer.height) return 0
    return layer.data[(ly * layer.width + lx) * 4 + 3]
  }
  // Half the neck's width (20 px) out on either side, within the collar's height;
  // the top keeps it under the front collar.
  assert.equal(alpha(front, 12, 40), 255)
  assert.equal(alpha(rest, 12, 40), 255)
  assert.equal(alpha(front, 68, 40), 255)
  // Below the collar the garment stays where it was.
  assert.equal(alpha(front, 12, 85), 0)
  assert.equal(alpha(rest, 12, 85), 255)
})

test('the collar beside the neck that the hair hangs over stays under the hair', () => {
  const neck = raster('neck', 20, 20, 40, 50, () => [245, 205, 190, 255])
  const topwear = raster('topwear', 0, 0, 80, 100, (_x, y) =>
    y >= 20 && y <= 35 ? [42, 45, 62, 255] : [184, 176, 196, 255],
  )
  // A lock down the left side, x 8..15.
  const hair = { ...raster('neck', 8, 0, 8, 60, () => [40, 30, 70, 255]), id: 'back-hair', role: 'back-hair' as const, group: 'head' as const }
  const output = splitHighCollarOcclusion([topwear, hair, neck], anchors)
  const front = output.find((layer) => layer.role === 'collar-front')!
  const rest = output.find((layer) => layer.role === 'topwear')!
  assert.equal(alphaAt(front, 12, 40), 0, 'not over the lock')
  assert.equal(alphaAt(rest, 12, 40), 255)
  assert.equal(alphaAt(front, 17, 40), 255, 'beside it, as before')
})

test('what the face hides at rest stays under the neck, so a raised chin shows neck, not the guessed garment', () => {
  const neck = raster('neck', 20, 20, 40, 50, () => [245, 205, 190, 255])
  const topwear = raster('topwear', 0, 0, 80, 100, (_x, y) =>
    y >= 20 && y <= 35 ? [42, 45, 62, 255] : [184, 176, 196, 255],
  )
  const face = { ...raster('neck', 20, 0, 40, 30, () => [250, 225, 215, 255]), id: 'face', role: 'face' as const, group: 'head' as const }
  const output = splitHighCollarOcclusion([topwear, neck, face], anchors)
  const front = output.find((layer) => layer.role === 'collar-front')
  const at = (layer: RasterLayer | undefined, x: number, y: number) => {
    if (!layer) return 0
    const lx = x - layer.left
    const ly = y - layer.top
    if (lx < 0 || ly < 0 || lx >= layer.width || ly >= layer.height) return 0
    return layer.data[(ly * layer.width + lx) * 4 + 3]
  }
  // Under the face (y < 30): never the front collar; below it, as before.
  assert.equal(at(front, 40, 25), 0)
  assert.equal(at(front, 25, 25), 0)
  assert.ok(output.some((layer) => layer.role === 'collar-front' && at(layer, 12, 40) > 0))
})

test('an open collar the decomposition drew over the neck: only cloth reaching in from outside goes in front', () => {
  const skin = [245, 205, 190, 255] as const
  const chest = [240, 212, 198, 255] as const // the shirt painted with the open chest's skin
  const portraitChest = [238, 214, 200, 255] as const
  const shirt = [250, 250, 252, 255] as const
  const band = [120, 30, 40, 255] as const
  const chain = [140, 60, 70, 255] as const // a dark chain the band's colour lies nearer than skin
  const neck = raster('neck', 20, 20, 40, 50, () => skin)
  const topwear = raster('topwear', 0, 0, 80, 100, (x, y) =>
    y >= 20 && y < 32 ? band : (x < 28 || x >= 52) && y >= 40 ? shirt : chest,
  )
  const face = { ...raster('neck', 20, 0, 40, 20, () => skin), id: 'face', role: 'face' as const }
  // The portrait: the neck bare where the band was drawn but for a chain on it, the open chest,
  // the lapels over the neck's sides.
  const reference = raster('topwear', 0, 0, 80, 100, (x, y) => {
    if (x >= 20 && x < 60 && y >= 20 && y < 70) {
      if (x >= 36 && x < 44 && y >= 24 && y < 28) return chain
      if ((x < 28 || x >= 52) && y >= 40) return shirt
      return y >= 40 ? portraitChest : skin
    }
    return y >= 40 ? shirt : skin
  })
  const output = splitHighCollarOcclusion([topwear, neck, face], anchors, reference)
  const front = output.find((l) => l.role === 'collar-front')
  const rear = output.find((l) => l.role === 'collar-back')!
  assert.ok(front, 'the lapels are a front collar')
  assert.equal(alphaAt(front!, 22, 55), 255, 'a lapel over the neck side, in front')
  assert.equal(alphaAt(front!, 40, 55), 0, 'the chest skin the shirt was painted with is not cloth in front')
  assert.equal(alphaAt(front!, 40, 26), 0, 'nor the chain, an island inside the neck')
  assert.equal(alphaAt(rear, 40, 26), 255, 'the band under the chain goes behind the neck')
})

test('a white collar on pale skin the neck layer copied: the top keeps it, so a moving neck shows cloth, not holes', () => {
  const skin = [245, 205, 190, 255] as const
  const juban = [250, 246, 240, 255] as const
  const fold = [248, 238, 230, 255] as const // a fold the neck layer copied from the picture
  const copied = (x: number, y: number) => x >= 29 && x < 33 && y >= 56 && y < 60
  const neck = raster('neck', 20, 20, 40, 50, (x, y) => (copied(x, y) ? fold : skin))
  const topwear = raster('topwear', 0, 0, 80, 100, () => juban)
  const face = { ...raster('neck', 20, 0, 40, 20, () => skin), id: 'face', role: 'face' as const }
  const reference = raster('topwear', 0, 0, 80, 100, (x, y) =>
    x >= 20 && x < 60 && y < 44 ? skin : copied(x, y) ? fold : juban,
  )
  const output = splitHighCollarOcclusion([topwear, neck, face], anchors, reference)
  const front = output.find((l) => l.role === 'collar-front')!
  const back = output.find((l) => l.role === 'collar-back')!
  const top = output.find((l) => l.role === 'topwear')!
  assert.equal(alphaAt(back, 30, 58), 255, 'the picture matches the neck there: behind it')
  assert.equal(alphaAt(top, 30, 58), 255, 'the top keeps what the picture shows')
  assert.equal(alphaAt(front, 40, 60), 255)
  assert.equal(alphaAt(top, 40, 60), 255, 'and keeps the front collar under it')
  assert.equal(alphaAt(top, 40, 28), 0, 'the guessed garment behind the bare neck is not kept')
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
