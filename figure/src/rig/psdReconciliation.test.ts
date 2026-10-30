import type {
  Anime25DSourceReference,
  RasterLayer,
} from './anime25dImportTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { analyzeAnime25DPsd, reconcileAnime25DPsd } from './psdReconciliation'

type Rgb = readonly [number, number, number]
type Box = readonly [number, number, number, number]

const SIZE = 200
const BACKDROP: Rgb = [250, 250, 250]
const SKIN: Rgb = [236, 200, 180]
const CLOTH: Rgb = [60, 110, 70]
const GEM: Rgb = [220, 40, 50]
const EARRING: Rgb = [120, 60, 200]
const INK: Rgb = [40, 30, 30]

function reference(
  paint: (fill: (box: Box, color: Rgb) => void) => void,
  backdrop: (x: number, y: number) => Rgb = () => BACKDROP,
  size = SIZE,
): Anime25DSourceReference {
  const data = new Uint8ClampedArray(size * size * 4)
  for (let y = 0; y < size; y += 1) {
    for (let x = 0; x < size; x += 1) {
      data.set([...backdrop(x, y), 255], (y * size + x) * 4)
    }
  }
  paint(([left, top, width, height], color) => {
    for (let y = top; y < top + height; y += 1) {
      for (let x = left; x < left + width; x += 1) {
        data.set([...color, 255], (y * size + x) * 4)
      }
    }
  })
  return { width: size, height: size, data }
}

function layer(role: string, [left, top, width, height]: Box, color: Rgb) {
  const data = new Uint8ClampedArray(width * height * 4)
  for (let index = 0; index < data.length; index += 4) {
    data.set([...color, 255], index)
  }
  return {
    id: role,
    role,
    sourceName: role,
    order: 0,
    side: null,
    group: 'body',
    left,
    top,
    width,
    height,
    data,
  } as RasterLayer
}

/** A portrait whose layers reproduce the reference exactly. */
function portrait(fill: (box: Box, color: Rgb) => void) {
  fill([60, 40, 80, 60], SKIN)
  fill([40, 100, 120, 80], CLOTH)
}

function baseLayers() {
  return [
    layer('face', [60, 40, 80, 60], SKIN),
    layer('topwear', [40, 100, 120, 80], CLOTH),
  ]
}

test('a faithful split reconciles without defects', () => {
  const result = reconcileAnime25DPsd(baseLayers(), reference(portrait))!
  assert.equal(result.status, 'reconciled')
  assert.equal(result.backgroundKnown, true)
  assert.ok(result.agreement > 0.99)
  assert.deepEqual(result.regions, [])
})

test('art hidden under a later layer is reported as buried', () => {
  const layers = [layer('neckwear', [90, 110, 20, 20], GEM), ...baseLayers()]
  const result = reconcileAnime25DPsd(
    layers,
    reference((fill) => {
      portrait(fill)
      fill([90, 110, 20, 20], GEM)
    }),
  )!
  assert.deepEqual(
    result.regions.map(({ kind, role, area }) => ({ kind, role, area })),
    [{ kind: 'buried', role: 'neckwear', area: 400 }],
  )
})

test('illustrated art that no layer carries is reported as missing', () => {
  const result = reconcileAnime25DPsd(
    baseLayers(),
    reference((fill) => {
      portrait(fill)
      fill([46, 70, 10, 24], EARRING)
    }),
  )!
  assert.deepEqual(
    result.regions.map(({ kind, area, box }) => ({ kind, area, box })),
    [
      {
        kind: 'missing',
        area: 240,
        box: { x: 46, y: 70, width: 10, height: 24 },
      },
    ],
  )
})

test('thin ear chains count on a large portrait while distant specks and regions below the pixel floor do not', () => {
  const layers = [
    layer('face', [240, 160, 320, 240], SKIN),
    layer('topwear', [160, 400, 480, 320], CLOTH),
    layer('ears', [220, 260, 20, 28], SKIN),
    layer('ears', [560, 260, 20, 28], SKIN),
    // An occluded ear still provides evidence for the chain beside it.
    layer('front-hair', [560, 260, 20, 28], INK),
  ]
  const source = reference((fill) => {
    for (const drawing of layers) {
      const color = drawing.role === 'topwear' ? CLOTH : drawing.role === 'front-hair' ? INK : SKIN
      fill([drawing.left, drawing.top, drawing.width, drawing.height], color)
    }
    fill([226, 288, 2, 32], EARRING)
    fill([566, 288, 2, 32], EARRING)
    fill([100, 300, 2, 32], EARRING)
    fill([233, 288, 1, 32], EARRING)
  }, undefined, 800)
  const analysis = analyzeAnime25DPsd(layers, source)!
  assert.ok(analysis.minArea > 64, 'the portrait-wide size floor exceeds the chain area')
  assert.deepEqual(
    analysis.regions.map(({ kind, area, box }) => ({ kind, area, box })),
    [
      { kind: 'missing', area: 64, box: { x: 226, y: 288, width: 2, height: 32 } },
      { kind: 'missing', area: 64, box: { x: 566, y: 288, width: 2, height: 32 } },
    ],
  )
})

test('a sliver along an ear\'s side is not an earring and keeps the portrait-wide floor', () => {
  const layers = [
    layer('face', [240, 160, 320, 240], SKIN),
    layer('topwear', [160, 400, 480, 320], CLOTH),
    layer('ears', [220, 260, 20, 28], SKIN),
  ]
  const source = reference((fill) => {
    for (const drawing of layers) {
      fill([drawing.left, drawing.top, drawing.width, drawing.height], drawing.role === 'topwear' ? CLOTH : SKIN)
    }
    // The ear drawn a little wider than its layer: within the ear's height, hanging nowhere.
    fill([216, 264, 4, 20], EARRING)
  }, undefined, 800)
  const analysis = analyzeAnime25DPsd(layers, source)!
  assert.ok(analysis.minArea > 80)
  assert.deepEqual(analysis.regions, [])
})

test('paint over the open backdrop is spurious, over enclosed pale art a mismatch', () => {
  const layers = [
    ...baseLayers(),
    layer('front-hair', [20, 20, 20, 30], INK),
    layer('back-hair', [80, 50, 20, 20], INK),
  ]
  const result = reconcileAnime25DPsd(
    layers,
    reference((fill) => {
      portrait(fill)
      // A backdrop-coloured highlight enclosed by the face, like pale skin.
      fill([80, 50, 20, 20], BACKDROP)
    }),
  )!
  const byRole = Object.fromEntries(
    result.regions.map((region) => [region.role, region.kind]),
  )
  assert.equal(byRole['front-hair'], 'spurious')
  assert.equal(byRole['back-hair'], 'mismatch')
})

test('a PSD split from another illustration is not reconciled', () => {
  const result = reconcileAnime25DPsd(
    baseLayers(),
    reference((fill) => {
      fill([60, 40, 80, 60], INK)
      fill([40, 100, 120, 80], EARRING)
    }),
  )!
  assert.equal(result.status, 'reference-mismatch')
  assert.deepEqual(result.regions, [])
  assert.equal(result.heatmap, null)
})

test('a scenic backdrop disables the backdrop-dependent verdicts', () => {
  const result = reconcileAnime25DPsd(
    [...baseLayers(), layer('front-hair', [20, 20, 20, 30], INK)],
    reference(
      (fill) => {
        portrait(fill)
        fill([4, 60, 12, 24], EARRING)
      },
      (x, y) => [(x * 7) % 256, (y * 5) % 256, ((x + y) * 3) % 256],
    ),
  )!
  assert.equal(result.backgroundKnown, false)
  assert.ok(
    result.regions.every(
      (region) => region.kind !== 'missing' && region.kind !== 'spurious',
    ),
  )
})

test('square padding outside the illustration is never judged', () => {
  const padded = reference(portrait)
  for (let y = 0; y < SIZE; y += 1) {
    for (let x = 0; x < 30; x += 1) padded.data[(y * SIZE + x) * 4 + 3] = 0
  }
  const result = reconcileAnime25DPsd(
    [...baseLayers(), layer('handwear', [0, 120, 30, 40], INK)],
    padded,
  )!
  assert.deepEqual(result.regions, [])
  // The overview crops to the illustration, not the padding beside it.
  assert.deepEqual([result.heatmap?.width, result.heatmap?.height], [136, 152])
})
