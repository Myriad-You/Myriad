import type { RasterLayer } from './anime25dImportTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { dropTopSkinOverNeck } from './topSkinOverNeck'

const NECK = [223, 173, 170, 255]
const WASH = [231, 189, 182, 175]
const LINE = [106, 94, 92, 88]
const COLLAR = [245, 245, 248, 255]
/** A cream shirt, as light as the skin: not skin. */
const CREAM = [240, 232, 220, 255]

function layer(id: string, role: RasterLayer['role'], box: { left: number; top: number; width: number; height: number }): RasterLayer {
  return { id, role, sourceName: id, order: 0, side: null, group: 'body', ...box, data: new Uint8ClampedArray(box.width * box.height * 4) }
}

function paint(target: RasterLayer, x0: number, y0: number, x1: number, y1: number, rgba: readonly number[]) {
  for (let y = y0; y < y1; y += 1) {
    for (let x = x0; x < x1; x += 1) target.data.set(rgba, ((y - target.top) * target.width + x - target.left) * 4)
  }
}

function alpha(target: RasterLayer, x: number, y: number): number {
  return target.data[((y - target.top) * target.width + x - target.left) * 4 + 3]
}

/** A neck over x 40..60, y 0..80; a top over it from y 40 with a wash of skin over the neck, its line, and a collar. */
function throat() {
  const neck = layer('neck', 'neck', { left: 40, top: 0, width: 20, height: 80 })
  paint(neck, 40, 0, 60, 80, NECK)
  const top = layer('topwear', 'topwear', { left: 0, top: 30, width: 100, height: 70 })
  paint(top, 0, 50, 100, 100, COLLAR)
  paint(top, 40, 56, 60, 60, CREAM)
  paint(top, 40, 41, 60, 50, WASH)
  paint(top, 40, 40, 60, 41, LINE)
  return [neck, top]
}

const always = () => true

test('the skin the top draws over the neck goes, and the line where it ends', () => {
  const [, top] = dropTopSkinOverNeck(throat(), always)
  assert.equal(alpha(top, 50, 45), 0, 'the wash')
  assert.equal(alpha(top, 50, 40), 0, 'its line')
})

test('a collar over the neck and the top beside it stay', () => {
  const [, top] = dropTopSkinOverNeck(throat(), always)
  assert.equal(alpha(top, 50, 55), 255, 'the collar over the neck')
  assert.equal(alpha(top, 50, 58), 255, 'a cream shirt over the neck')
  assert.equal(alpha(top, 20, 70), 255, 'the top off the neck')
})

test('a speck of the collar the colour of the skin, apart from the skin, stays', () => {
  const [neck, top] = throat()
  paint(top, 42, 70, 44, 72, WASH.slice(0, 3).concat(255))
  const [, after] = dropTopSkinOverNeck([neck, top], always)
  assert.equal(alpha(after, 43, 71), 255)
})

test('a top under the neck, or no neck, is left', () => {
  const [neck, top] = throat()
  const below = [top, neck]
  assert.deepEqual(dropTopSkinOverNeck(below, always), below)
  assert.equal(dropTopSkinOverNeck([top], always)[0], top)
})
