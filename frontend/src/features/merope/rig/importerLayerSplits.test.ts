import type { RasterLayer } from './anime25dImportTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { stackArmsByReference, stackHeadwearByReference, stackLowerLimbsByReference, stackNeckwearByReference } from './importerLayerSplits'

const SIZE = 20

function solid(
  id: string,
  role: RasterLayer['role'],
  rgb: readonly [number, number, number],
  box: { left: number; top: number; width: number; height: number },
): RasterLayer {
  const data = new Uint8ClampedArray(box.width * box.height * 4)
  for (let i = 0; i < data.length; i += 4) data.set([...rgb, 255], i)
  return {
    id,
    role,
    sourceName: id,
    order: 0,
    side: 'left',
    group: 'body',
    ...box,
    data,
  }
}

/** The portrait: a white sock above y = 12, a violet shoe from there down. */
function portrait(shoeTop: number) {
  const data = new Uint8ClampedArray(SIZE * SIZE * 4)
  for (let y = 0; y < SIZE; y += 1) {
    for (let x = 0; x < SIZE; x += 1) {
      data.set(y < shoeTop ? [250, 250, 250, 255] : [120, 90, 200, 255], (y * SIZE + x) * 4)
    }
  }
  return { width: SIZE, height: SIZE, data }
}

test('a shoe the portrait shows over the sock is stacked above it', () => {
  const shoe = solid('shoe', 'footwear', [120, 90, 200], { left: 0, top: 12, width: SIZE, height: 8 })
  // The sock runs down into the shoe and was put on top of it.
  const sock = solid('sock', 'legwear', [250, 250, 250], { left: 0, top: 0, width: SIZE, height: 16 })
  const stacked = stackLowerLimbsByReference([shoe, sock], portrait(12))
  assert.deepEqual(stacked.map((layer) => layer.id), ['sock', 'shoe'])
})

test('a hem the portrait shows over the shoe stays on top', () => {
  const shoe = solid('shoe', 'footwear', [120, 90, 200], { left: 0, top: 12, width: SIZE, height: 8 })
  const hem = solid('hem', 'legwear', [250, 250, 250], { left: 0, top: 0, width: SIZE, height: 16 })
  // The trouser hem covers the top of the shoe in the portrait.
  const stacked = stackLowerLimbsByReference([shoe, hem], portrait(16))
  assert.deepEqual(stacked.map((layer) => layer.id), ['shoe', 'hem'])
})

test('a pair the portrait cannot tell apart keeps its order', () => {
  const shoe = solid('shoe', 'footwear', [250, 250, 250], { left: 0, top: 12, width: SIZE, height: 8 })
  const sock = solid('sock', 'legwear', [250, 250, 250], { left: 0, top: 0, width: SIZE, height: 16 })
  const stacked = stackLowerLimbsByReference([shoe, sock], portrait(12))
  assert.deepEqual(stacked.map((layer) => layer.id), ['shoe', 'sock'])
})

/** The portrait: skin, a violet choker band at y 8..10, a pink pendant on the white top below. */
function neckPortrait() {
  const data = new Uint8ClampedArray(SIZE * SIZE * 4)
  for (let y = 0; y < SIZE; y += 1) {
    for (let x = 0; x < SIZE; x += 1) {
      const pendant = y >= 12 && y < 16 && x >= 4 && x < 16
      const colour = y >= 8 && y < 10 ? [120, 90, 200] : pendant ? [240, 120, 170] : y < 10 ? [245, 200, 190] : [250, 250, 250]
      data.set([...colour, 255], (y * SIZE + x) * 4)
    }
  }
  return { width: SIZE, height: SIZE, data }
}

function choker(): RasterLayer {
  const layer = solid('choker', 'neckwear', [120, 90, 200], { left: 0, top: 8, width: SIZE, height: 8 })
  for (let y = 0; y < 8; y += 1) {
    for (let x = 0; x < SIZE; x += 1) {
      const i = (y * SIZE + x) * 4
      if (y >= 2) layer.data.set(y >= 4 && x >= 4 && x < 16 ? [240, 120, 170, 255] : [0, 0, 0, 0], i)
    }
  }
  return layer
}

test('a choker put behind the neck and the top it lies on is stacked over both', () => {
  // The neck's own picture runs down under the choker with a faded band of it.
  const neck = solid('neck', 'neck', [245, 200, 190], { left: 0, top: 0, width: SIZE, height: 12 })
  const top = solid('top', 'topwear', [250, 250, 250], { left: 0, top: 10, width: SIZE, height: 10 })
  const stacked = stackNeckwearByReference([choker(), neck, top], neckPortrait())
  assert.deepEqual(stacked.map((layer) => layer.id), ['neck', 'top', 'choker'])
})

test('neckwear the portrait shows under the top stays under it', () => {
  const neck = solid('neck', 'neck', [245, 200, 190], { left: 0, top: 0, width: SIZE, height: 10 })
  const scarf = solid('scarf', 'neckwear', [240, 120, 170], { left: 0, top: 10, width: SIZE, height: 10 })
  const top = solid('top', 'topwear', [250, 250, 250], { left: 0, top: 10, width: SIZE, height: 10 })
  // A white top covering a pink scarf: where they overlap the portrait is white.
  const stacked = stackNeckwearByReference([neck, scarf, top], neckPortrait())
  assert.deepEqual(stacked.map((layer) => layer.id), ['neck', 'scarf', 'top'])
})

/** The portrait: a violet skirt over columns 4..16, white around it. */
function skirtPortrait() {
  const data = new Uint8ClampedArray(SIZE * SIZE * 4)
  for (let y = 0; y < SIZE; y += 1) {
    for (let x = 0; x < SIZE; x += 1) {
      data.set(x >= 4 && x < 16 ? [120, 90, 200, 255] : [250, 250, 250, 255], (y * SIZE + x) * 4)
    }
  }
  return { width: SIZE, height: SIZE, data }
}

test('an arm hanging beside the skirt goes in front of it and the legs', () => {
  const skirt = solid('skirt', 'bottomwear', [120, 90, 200], { left: 4, top: 0, width: 12, height: 12 })
  const leg = solid('leg', 'legwear', [245, 200, 190], { left: 6, top: 10, width: 8, height: 10 })
  // The hand only grazes the skirt's edge: too little to tell.
  const hand = solid('hand', 'handwear', [250, 250, 250], { left: 0, top: 0, width: 5, height: 12 })
  const stacked = stackArmsByReference([hand, leg, skirt], skirtPortrait())
  assert.deepEqual(stacked.map((layer) => layer.id), ['leg', 'skirt', 'hand'])
})

test('a sleeve the portrait shows tucked behind the skirt stays behind it', () => {
  const skirt = solid('skirt', 'bottomwear', [120, 90, 200], { left: 4, top: 0, width: 12, height: 12 })
  // The sleeve's hidden inner side runs under the skirt, where the portrait shows the skirt.
  const sleeve = solid('sleeve', 'handwear', [250, 250, 250], { left: 0, top: 0, width: 10, height: 12 })
  const stacked = stackArmsByReference([sleeve, skirt], skirtPortrait())
  assert.deepEqual(stacked.map((layer) => layer.id), ['sleeve', 'skirt'])
})

/** The portrait: pink hair, with a gold clip over columns 6..14 of rows 4..10. */
function clipPortrait() {
  const data = new Uint8ClampedArray(SIZE * SIZE * 4)
  for (let y = 0; y < SIZE; y += 1) {
    for (let x = 0; x < SIZE; x += 1) {
      const clip = x >= 6 && x < 14 && y >= 4 && y < 10
      data.set(clip ? [220, 180, 90, 255] : [250, 170, 190, 255], (y * SIZE + x) * 4)
    }
  }
  return { width: SIZE, height: SIZE, data }
}

test('a hair clip the portrait shows over the front hair is stacked above it', () => {
  const clip = solid('clip', 'headwear', [220, 180, 90], { left: 6, top: 4, width: 8, height: 6 })
  const hair = solid('hair', 'front-hair', [250, 170, 190], { left: 0, top: 0, width: SIZE, height: SIZE })
  const stacked = stackHeadwearByReference([clip, hair], clipPortrait())
  assert.deepEqual(stacked.map((layer) => layer.id), ['hair', 'clip'])
})

test('a hat brim the portrait shows under the bangs stays under them', () => {
  const brim = solid('brim', 'headwear', [220, 180, 90], { left: 0, top: 0, width: SIZE, height: SIZE })
  // Bangs fall over the brim's middle: there the portrait is hair.
  const bangs = solid('bangs', 'front-hair', [250, 170, 190], { left: 0, top: 10, width: SIZE, height: 10 })
  const portrait = clipPortrait()
  for (let y = 10; y < SIZE; y += 1) {
    for (let x = 0; x < SIZE; x += 1) portrait.data.set([250, 170, 190, 255], (y * SIZE + x) * 4)
  }
  const stacked = stackHeadwearByReference([brim, bangs], portrait)
  assert.deepEqual(stacked.map((layer) => layer.id), ['brim', 'bangs'])
})
