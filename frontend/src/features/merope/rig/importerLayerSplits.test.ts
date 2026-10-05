import type { RasterLayer } from './anime25dImportTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { stackLowerLimbsByReference, stackNeckwearByReference } from './importerLayerSplits'

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
