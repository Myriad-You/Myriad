import type { RasterLayer } from './anime25dImportTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { hangTopPiecesOnHeadwear } from './headOrnaments'

const SHIRT = [240, 240, 245, 255]
const RIBBON = [200, 225, 235, 255]
const CLIP = [230, 200, 160, 255]

function layer(id: string, role: RasterLayer['role'], box: { left: number; top: number; width: number; height: number }): RasterLayer {
  return { id, role, sourceName: id, order: 0, side: null, group: role === 'topwear' ? 'body' : 'head', ...box, data: new Uint8ClampedArray(box.width * box.height * 4) }
}

function paint(target: RasterLayer, x0: number, y0: number, x1: number, y1: number, rgba: readonly number[]) {
  for (let y = y0; y < y1; y += 1) {
    for (let x = x0; x < x1; x += 1) target.data.set(rgba, ((y - target.top) * target.width + x - target.left) * 4)
  }
}

function pixel(target: RasterLayer, x: number, y: number): number[] {
  const col = x - target.left
  const row = y - target.top
  if (col < 0 || row < 0 || col >= target.width || row >= target.height) return [0, 0, 0, 0]
  return Array.from(target.data.subarray((row * target.width + col) * 4, (row * target.width + col) * 4 + 4))
}

/** A shirt (y 60..100), and a ribbon in its layer (x 70..76, y 22..58) hanging from a clip (y 10..22). */
function clipAndRibbon() {
  const top = layer('topwear', 'topwear', { left: 0, top: 0, width: 100, height: 100 })
  paint(top, 0, 60, 100, 100, SHIRT)
  paint(top, 70, 22, 76, 58, RIBBON)
  const clip = layer('headwear', 'headwear', { left: 65, top: 10, width: 15, height: 12 })
  paint(clip, 65, 10, 80, 22, CLIP)
  return [top, clip]
}

const always = () => true

test('a ribbon the top took goes to the clip it hangs from', () => {
  const [top, clip] = hangTopPiecesOnHeadwear(clipAndRibbon(), always)
  assert.equal(pixel(top, 72, 40)[3], 0, 'out of the top')
  assert.deepEqual(pixel(clip, 72, 40), RIBBON, 'into the clip')
  assert.deepEqual(pixel(clip, 70, 15), CLIP, 'the clip as it was')
  assert.deepEqual(pixel(top, 50, 80), SHIRT, 'the shirt stays')
})

test('a piece of the top that hangs from nothing, or that a layer between would uncover, stays', () => {
  const [top] = clipAndRibbon()
  const away = layer('headwear', 'headwear', { left: 10, top: 10, width: 10, height: 10 })
  paint(away, 10, 10, 20, 20, CLIP)
  assert.equal(hangTopPiecesOnHeadwear([top, away], always)[0], top)
  const [top2, clip2] = clipAndRibbon()
  const hair = layer('back-hair', 'back-hair', { left: 0, top: 0, width: 100, height: 100 })
  paint(hair, 70, 30, 76, 35, [120, 80, 60, 255])
  const [topAfter] = hangTopPiecesOnHeadwear([top2, hair, clip2], always)
  assert.equal(topAfter, top2)
})
