import type {
  Anime25DSourceReference,
  RasterLayer,
} from './anime25dImportTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { repairAnime25DPsd } from './psdRepair'

type Rgb = readonly [number, number, number]
type Box = readonly [number, number, number, number]

const SIZE = 200
const BACKDROP: Rgb = [250, 250, 250]
const SKIN: Rgb = [236, 200, 180]
const CLOTH: Rgb = [60, 110, 70]
const HAIR: Rgb = [200, 170, 90]
const GOLD: Rgb = [90, 50, 20]
const GEM: Rgb = [220, 40, 50]
const allVisible = () => true

function reference(
  paint: (fill: (box: Box, color: Rgb) => void) => void,
): Anime25DSourceReference {
  const data = new Uint8ClampedArray(SIZE * SIZE * 4)
  for (let index = 0; index < data.length; index += 4) {
    data.set([...BACKDROP, 255], index)
  }
  paint(([left, top, width, height], color) => {
    for (let y = top; y < top + height; y += 1) {
      for (let x = left; x < left + width; x += 1) {
        data.set([...color, 255], (y * SIZE + x) * 4)
      }
    }
  })
  return { width: SIZE, height: SIZE, data }
}

function layer(
  role: string,
  [left, top, width, height]: Box,
  color: Rgb,
  group: RasterLayer['group'] = 'body',
): RasterLayer {
  const data = new Uint8ClampedArray(width * height * 4)
  for (let index = 0; index < data.length; index += 4) {
    data.set([...color, 255], index)
  }
  return {
    id: role,
    role: role as RasterLayer['role'],
    sourceName: role,
    order: 0,
    side: null,
    group,
    left,
    top,
    width,
    height,
    data,
  }
}

function pixel(
  layer: RasterLayer,
  x: number,
  y: number,
): [number, number, number, number] {
  const offset = ((y - layer.top) * layer.width + (x - layer.left)) * 4
  return [...layer.data.subarray(offset, offset + 4)] as [
    number,
    number,
    number,
    number,
  ]
}

const FACE: Box = [60, 20, 80, 70]
const TORSO: Box = [40, 100, 120, 80]

function portrait(fill: (box: Box, color: Rgb) => void) {
  fill(FACE, SKIN)
  fill(TORSO, CLOTH)
}

test('covered art is revealed by erasing only the covering pixels', () => {
  const arm = layer('handwear', [100, 120, 40, 40], SKIN)
  const torso = layer('topwear', TORSO, CLOTH)
  const layers = [layer('face', FACE, SKIN, 'head'), arm, torso]
  const result = repairAnime25DPsd(
    layers,
    allVisible,
    reference((fill) => {
      portrait(fill)
      fill([100, 120, 40, 40], SKIN)
    }),
    2,
  )
  const repairedTorso = result.layers[2]
  assert.notEqual(repairedTorso, torso, 'edits never touch the input layer')
  assert.equal(pixel(torso, 120, 140)[3], 255)
  assert.equal(pixel(repairedTorso, 120, 140)[3], 0)
  assert.equal(pixel(repairedTorso, 60, 140)[3], 255)
  assert.equal(result.layers[1], arm)
  assert.deepEqual(result.reconciliation?.repairs, {
    revealed: 1,
    recovered: 0,
  })
  assert.deepEqual(result.reconciliation?.regions, [])
})

function paint(
  target: RasterLayer,
  [left, top, width, height]: Box,
  color: Rgb,
  alpha = 255,
) {
  for (let y = top; y < top + height; y += 1) {
    for (let x = left; x < left + width; x += 1) {
      target.data.set(
        [...color, alpha],
        ((y - target.top) * target.width + (x - target.left)) * 4,
      )
    }
  }
}

test('a reveal grows to the covering art edge, not the verdict threshold', () => {
  const arm = layer('handwear', [80, 110, 60, 40], SKIN)
  const torso = layer('topwear', TORSO, CLOTH)
  // A wide band close enough to skin to escape the verdict, still clearly worse.
  const band: Rgb = [SKIN[0] - 50, SKIN[1] - 50, SKIN[2] - 40]
  paint(torso, [120, 110, 20, 40], band)
  const result = repairAnime25DPsd(
    [layer('face', FACE, SKIN, 'head'), arm, torso],
    allVisible,
    reference((fill) => {
      portrait(fill)
      fill([80, 110, 60, 40], SKIN)
    }),
    2,
  )
  const repaired = result.layers[2]
  assert.equal(pixel(repaired, 90, 130)[3], 0)
  assert.equal(pixel(repaired, 137, 130)[3], 0, 'the far side of the band')
  assert.equal(pixel(repaired, 150, 130)[3], 255, 'beyond the covered arm')
})

test('the removed drawing leaves no anti-aliased ghost outline', () => {
  const arm = layer('handwear', [80, 110, 60, 40], SKIN)
  const hair = layer('back-hair', [90, 115, 30, 30], HAIR, 'head')
  paint(hair, [90, 115, 30, 1], HAIR, 20)
  const result = repairAnime25DPsd(
    [
      layer('face', FACE, SKIN, 'head'),
      layer('topwear', TORSO, CLOTH),
      arm,
      hair,
    ],
    allVisible,
    reference((fill) => {
      portrait(fill)
      fill([80, 110, 60, 40], SKIN)
    }),
    2,
  )
  const repaired = result.layers[3]
  assert.equal(pixel(repaired, 100, 130)[3], 0)
  assert.equal(pixel(repaired, 100, 115)[3], 0, 'the translucent rim')
})

test('layers with expression variants are neither edited nor covered', () => {
  const face = layer('face', FACE, SKIN, 'head')
  const lash = layer('eyelash', [80, 40, 40, 12], GOLD, 'head')
  const result = repairAnime25DPsd(
    [face, lash],
    allVisible,
    reference((fill) => {
      portrait(fill)
      fill([80, 40, 40, 12], SKIN)
    }),
    2,
  )
  assert.equal(result.layers[1], lash)
  assert.deepEqual(result.reconciliation?.repairs, {
    revealed: 0,
    recovered: 0,
  })
  assert.equal(result.reconciliation?.regions[0]?.kind, 'buried')
})

test('a covered layer that matches only by coincidence is recovered instead', () => {
  // Every pixel is within the match distance, yet the art is not the same.
  const near: Rgb = [SKIN[0] - 30, SKIN[1] - 30, SKIN[2]]
  const torso = layer('topwear', TORSO, CLOTH)
  const result = repairAnime25DPsd(
    [
      layer('face', FACE, SKIN, 'head'),
      layer('handwear', [110, 130, 16, 16], near),
      torso,
    ],
    allVisible,
    reference((fill) => {
      portrait(fill)
      fill([110, 130, 16, 16], SKIN)
    }),
    2,
  )
  assert.equal(result.layers[2], torso, 'the torso is left intact')
  assert.equal(result.layers[3]?.id, 'recovered-objects')
  const recovered = result.layers.find((candidate) =>
    candidate.id.startsWith('recovered-'),
  )!
  assert.equal(recovered.role, 'objects')
  assert.deepEqual(pixel(recovered, 118, 138), [...SKIN, 255])
  assert.deepEqual(result.reconciliation?.repairs, {
    revealed: 0,
    recovered: 1,
  })
})

test('missing art is lifted from the source into a group layer above its neighbours', () => {
  const face = layer('face', FACE, SKIN, 'head')
  const result = repairAnime25DPsd(
    [face, layer('topwear', TORSO, CLOTH)],
    allVisible,
    reference((fill) => {
      portrait(fill)
      fill([50, 60, 10, 24], GEM)
    }),
    2,
  )
  const recovered = result.layers.find((candidate) =>
    candidate.id.startsWith('recovered-'),
  )!
  assert.equal(recovered.group, 'head')
  assert.equal(result.layers.indexOf(recovered), 1, 'right above the face')
  assert.deepEqual(pixel(recovered, 55, 70), [...GEM, 255])
  assert.deepEqual(result.reconciliation?.regions, [])
  assert.equal(result.reconciliation?.repairs.recovered, 1)
})

test('a bow as big as a garment piece is recovered when it hangs from the hair above the chin', () => {
  const face = layer('face', FACE, SKIN, 'head')
  const hair = layer('back-hair', [40, 10, 20, 60], HAIR, 'head')
  const result = repairAnime25DPsd(
    [hair, face, layer('topwear', TORSO, CLOTH)],
    allVisible,
    reference((fill) => {
      portrait(fill)
      fill([40, 10, 20, 60], HAIR)
      // 600 px: past 2% of the 16400 compared, within 10%.
      fill([20, 20, 20, 30], GEM)
    }),
    2,
  )
  const recovered = result.layers.find((candidate) =>
    candidate.id.startsWith('recovered-'),
  )
  assert.ok(recovered, 'the bow is lifted')
  assert.equal(recovered.group, 'head')
  assert.equal(recovered.role, 'headwear')
  assert.deepEqual(pixel(recovered, 30, 35), [...GEM, 255])
})

test('an accessory broken into thin parts is recovered whole', () => {
  const hair = layer('back-hair', [40, 20, 120, 80], HAIR, 'head')
  const result = repairAnime25DPsd(
    [hair, layer('topwear', TORSO, CLOTH)],
    allVisible,
    reference((fill) => {
      fill([40, 20, 120, 80], HAIR)
      fill(TORSO, CLOTH)
      // Cage bars over the hair, with a separate base plate below them.
      for (let x = 90; x <= 100; x += 5) fill([x, 50, 1, 12], GOLD)
      fill([88, 64, 14, 4], GOLD)
    }),
    2,
  )
  const recovered = result.layers.find((candidate) =>
    candidate.id.startsWith('recovered-'),
  )!
  assert.equal(recovered.group, 'head')
  assert.equal(result.layers.indexOf(recovered), 1)
  assert.deepEqual(pixel(recovered, 90, 55), [...GOLD, 255])
  assert.deepEqual(
    pixel(recovered, 92, 55),
    [...HAIR, 255],
    'hair seen between the bars belongs to the accessory',
  )
  assert.equal(result.reconciliation?.repairs.recovered, 1)
})

test('a piece hanging from an accessory joins that drawing', () => {
  const ornament = layer('headwear', [150, 30, 10, 20], GOLD, 'head')
  const result = repairAnime25DPsd(
    [
      layer('face', FACE, SKIN, 'head'),
      ornament,
      layer('topwear', TORSO, CLOTH),
    ],
    allVisible,
    reference((fill) => {
      portrait(fill)
      fill([150, 30, 10, 20], GOLD)
      // A tassel below the ornament that the split dropped.
      fill([151, 50, 8, 30], GEM)
    }),
    2,
  )
  assert.equal(result.layers.length, 3, 'no new layer')
  const joined = result.layers[1]
  assert.equal(joined.role, 'headwear')
  assert.deepEqual(pixel(joined, 155, 40), [...GOLD, 255])
  assert.deepEqual(pixel(joined, 155, 70), [...GEM, 255])
  assert.equal(pixel(ornament, 155, 40)[3], 255)
  assert.equal(ornament.height, 20, 'the input layer is untouched')
  assert.equal(result.reconciliation?.repairs.recovered, 1)
})

test('a dropped earring hangs from the ear', () => {
  const result = repairAnime25DPsd(
    [
      layer('ears', [52, 50, 10, 16], SKIN, 'head'),
      layer('face', FACE, SKIN, 'head'),
      layer('topwear', TORSO, CLOTH),
    ],
    allVisible,
    reference((fill) => {
      fill([52, 50, 10, 16], SKIN)
      portrait(fill)
      fill([50, 66, 10, 24], GEM)
    }),
    2,
  )
  const earring = result.layers.find((candidate) =>
    candidate.id.startsWith('recovered-'),
  )!
  assert.equal(earring.role, 'earwear')
  assert.equal(earring.group, 'head')
})

test('without a free layer slot recoveries are only reported', () => {
  const layers = [
    layer('face', FACE, SKIN, 'head'),
    layer('topwear', TORSO, CLOTH),
  ]
  const result = repairAnime25DPsd(
    layers,
    allVisible,
    reference((fill) => {
      portrait(fill)
      fill([50, 60, 10, 24], GEM)
    }),
    0,
  )
  assert.deepEqual(result.layers, layers)
  assert.equal(result.reconciliation?.regions[0]?.kind, 'missing')
})

test('garment-sized disagreement is reported, not pasted over', () => {
  const layers = [
    layer('face', FACE, SKIN, 'head'),
    layer('topwear', TORSO, CLOTH),
  ]
  const result = repairAnime25DPsd(
    layers,
    allVisible,
    reference((fill) => {
      portrait(fill)
      fill([60, 120, 40, 40], GEM)
    }),
    2,
  )
  assert.deepEqual(result.layers, layers)
  assert.equal(result.reconciliation?.status, 'reconciled')
  assert.equal(result.reconciliation?.repairs.recovered, 0)
  assert.equal(result.reconciliation?.regions[0]?.kind, 'mismatch')
})

test('a PSD split from another illustration is left untouched', () => {
  const layers = [
    layer('face', FACE, SKIN, 'head'),
    layer('topwear', TORSO, CLOTH),
  ]
  const result = repairAnime25DPsd(
    layers,
    allVisible,
    reference((fill) => {
      fill(FACE, GOLD)
      fill(TORSO, GEM)
    }),
    2,
  )
  assert.deepEqual(result.layers, layers)
  assert.equal(result.reconciliation?.status, 'reference-mismatch')
})
