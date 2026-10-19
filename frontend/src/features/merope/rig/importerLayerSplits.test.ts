import type { RasterLayer } from './anime25dImportTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { stackArmsByReference, stackEarwearByReference, stackHeadwearByReference, stackLowerLimbsByReference, stackNeckwearByReference, tuckBackHairBehindFace, tuckBackHairBehindNeck, tuckTopUnderChinBehindNeck } from './importerLayerSplits'

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

test('an earring the portrait shows over the side locks is stacked above them', () => {
  const earring = solid('earring', 'earwear', [220, 180, 90], { left: 6, top: 4, width: 8, height: 6 })
  const locks = solid('locks', 'front-hair', [250, 170, 190], { left: 0, top: 0, width: SIZE, height: SIZE })
  const stacked = stackEarwearByReference([earring, locks], clipPortrait())
  assert.deepEqual(stacked.map((layer) => layer.id), ['locks', 'earring'])
  const hidden = solid('hidden', 'earwear', [220, 180, 90], { left: 0, top: 12, width: 8, height: 6 })
  assert.deepEqual(stackEarwearByReference([hidden, locks], clipPortrait()).map((layer) => layer.id), ['hidden', 'locks'])
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

test('back hair put over a neck drawn over the top goes just behind the neck, still over the top', () => {
  const box = { left: 0, top: 0, width: 4, height: 4 }
  const topwear = solid('topwear', 'topwear', [40, 40, 90], box)
  const collarBack = solid('collar-back', 'collar-back', [40, 40, 90], box)
  const neck = solid('neck', 'neck', [240, 210, 200], box)
  const collarFront = solid('collar-front', 'collar-front', [40, 40, 90], box)
  const hair = solid('back-hair', 'back-hair', [30, 30, 60], box)
  const face = solid('face', 'face', [250, 225, 215], box)
  const out = tuckBackHairBehindNeck([topwear, collarBack, neck, collarFront, hair, face])
  assert.deepEqual(out.map((l) => l.id), ['topwear', 'collar-back', 'back-hair', 'neck', 'collar-front', 'face'])
  // Already behind the neck, or a neck under the top (the runtime lifts it): as it was.
  const behind = [hair, topwear, neck, face]
  assert.equal(tuckBackHairBehindNeck(behind), behind)
  const under = [neck, topwear, hair, face]
  assert.equal(tuckBackHairBehindNeck(under), under)
})

test('back hair between a neck and the stand collar drawn over it goes behind the neck', () => {
  const box = { left: 0, top: 0, width: 4, height: 4 }
  const neck = solid('neck', 'neck', [240, 210, 200], box)
  const hair = solid('back-hair', 'back-hair', [30, 30, 60], box)
  const topwear = solid('topwear', 'topwear', [40, 40, 90], box)
  const face = solid('face', 'face', [250, 225, 215], box)
  const out = tuckBackHairBehindNeck([neck, hair, topwear, face])
  assert.deepEqual(out.map((l) => l.id), ['back-hair', 'neck', 'topwear', 'face'])
})

test('back hair stacked over the face goes just behind it', () => {
  const box = { left: 0, top: 0, width: 4, height: 4 }
  const ears = solid('ears', 'ears', [240, 210, 200], box)
  const face = solid('face', 'face', [250, 225, 215], box)
  const hair = solid('back-hair', 'back-hair', [200, 150, 150], box)
  const nose = solid('nose', 'nose', [240, 200, 190], box)
  const bangs = solid('front-hair', 'front-hair', [250, 170, 190], box)
  const out = tuckBackHairBehindFace([ears, face, hair, nose, bangs])
  assert.deepEqual(out.map((l) => l.id), ['ears', 'back-hair', 'face', 'nose', 'front-hair'])
  // Already behind the face, or no face: as it was.
  const behind = [hair, ears, face, nose]
  assert.equal(tuckBackHairBehindFace(behind), behind)
  const faceless = [ears, hair]
  assert.equal(tuckBackHairBehindFace(faceless), faceless)
})

test('what a top draws under the chin, in front of a neck that shows below the jaw, goes behind the neck', () => {
  // A face down to y 10, the neck from y 4 down, a hood in front of the neck with a hole below the jaw.
  const neck = solid('neck', 'neck', [240, 210, 200], { left: 6, top: 4, width: 8, height: 16 })
  const hood = solid('topwear', 'topwear', [40, 30, 30], { left: 0, top: 6, width: SIZE, height: 14 })
  for (let y = 11; y < 20; y += 1) {
    for (let x = 7; x < 13; x += 1) hood.data[((y - 6) * SIZE + x) * 4 + 3] = 0
  }
  const face = solid('face', 'face', [250, 225, 215], { left: 4, top: 0, width: 12, height: 11 })
  const out = tuckTopUnderChinBehindNeck([neck, hood, face], () => true)
  assert.deepEqual(out.map((l) => l.id), ['topwear-lining', 'neck', 'topwear', 'face'])
  const [lining, , front] = out
  // Under the chin over the neck, the hood's pixel is behind the neck now; beside the neck it stays.
  assert.equal(front.data[((8 - 6) * SIZE + 9) * 4 + 3], 0)
  assert.equal(lining.data[((8 - lining.top) * lining.width + 9 - lining.left) * 4 + 3], 255)
  assert.equal(front.data[((8 - 6) * SIZE + 2) * 4 + 3], 255)
  // A top the portrait shows right under the jaw (a closed collar) stays in front.
  const closed = solid('topwear', 'topwear', [40, 30, 30], { left: 0, top: 6, width: SIZE, height: 14 })
  const kept = [neck, closed, face]
  assert.equal(tuckTopUnderChinBehindNeck(kept, () => true), kept)
})
