import type { RasterLayer } from './anime25dImportTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { splitTrouserLegsIfNeeded } from './trouserLegs'

function solid(
  id: string,
  role: RasterLayer['role'],
  rgb: readonly [number, number, number],
  box: { left: number; top: number; width: number; height: number },
): RasterLayer {
  const data = new Uint8ClampedArray(box.width * box.height * 4)
  for (let i = 0; i < data.length; i += 4) data.set([...rgb, 255], i)
  return { id, role, sourceName: id, order: 0, side: null, group: 'body', ...box, data }
}

/** Trousers 40 wide and 100 tall: closed above row 40, then two legs with a gap at x 17..22 (or none: a skirt). */
function trousers(gap: boolean): RasterLayer {
  const layer = solid('trousers', 'bottomwear', [40, 40, 50], { left: 0, top: 0, width: 40, height: 100 })
  if (!gap) return layer
  for (let y = 40; y < 100; y += 1) {
    for (let x = 17; x < 23; x += 1) layer.data[(y * 40 + x) * 4 + 3] = 0
  }
  return layer
}

function alphaAt(layer: RasterLayer, x: number, y: number): number {
  const inside = x >= layer.left && y >= layer.top && x < layer.left + layer.width && y < layer.top + layer.height
  return inside ? layer.data[((y - layer.top) * layer.width + (x - layer.left)) * 4 + 3] : 0
}

test('long trousers down to the shoes are the legs below the crotch, the seat fading out over them', () => {
  const shoes = solid('shoes', 'footwear', [250, 250, 250], { left: 0, top: 95, width: 40, height: 5 })
  const split = splitTrouserLegsIfNeeded([trousers(true), shoes])
  assert.deepEqual(split.map((layer) => layer.role), ['legwear', 'bottomwear', 'footwear'])
  const [legs, seat] = split
  // In the middle the legs start just above the crotch, under the seat; at the sides up by the hips.
  assert.equal(alphaAt(legs, 20, 30), 0)
  assert.equal(alphaAt(legs, 15, 36), 255)
  assert.equal(alphaAt(legs, 0, 10), 255)
  assert.equal(alphaAt(legs, 5, 90), 255)
  assert.equal(alphaAt(seat, 20, 10), 255)
  assert.ok(alphaAt(seat, 15, 36) > 0 && alphaAt(seat, 15, 36) < 255, 'the seat fades over the legs')
  assert.equal(alphaAt(seat, 5, 90), 0)
})

test('a skirt, closed between the legs, and trousers with legs already are left as they are', () => {
  const shoes = solid('shoes', 'footwear', [250, 250, 250], { left: 0, top: 95, width: 40, height: 5 })
  const skirt = [trousers(false), shoes]
  assert.equal(splitTrouserLegsIfNeeded(skirt), skirt)
  const legs = solid('legs', 'legwear', [230, 200, 190], { left: 0, top: 60, width: 40, height: 40 })
  const dressed = [legs, trousers(true), shoes]
  assert.equal(splitTrouserLegsIfNeeded(dressed), dressed)
  // Shorts well above the shoes are not long trousers.
  const high = solid('shoes', 'footwear', [250, 250, 250], { left: 0, top: 140, width: 40, height: 5 })
  const shorts = [trousers(true), high]
  assert.equal(splitTrouserLegsIfNeeded(shorts), shorts)
})
