import type { RasterLayer } from './anime25dImportTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { underlapHairLocks } from './hairLockUnderlap'

function lock(
  id: string,
  rgb: readonly [number, number, number],
  box: { left: number; top: number; width: number; height: number },
  role: RasterLayer['role'] = 'front-hair',
): RasterLayer {
  const data = new Uint8ClampedArray(box.width * box.height * 4)
  for (let i = 0; i < data.length; i += 4) data.set([...rgb, 255], i)
  return { id, role, sourceName: id, order: 0, side: null, group: 'head', ...box, data }
}

function at(layer: RasterLayer, x: number, y: number): number[] {
  const lx = x - layer.left
  const ly = y - layer.top
  if (lx < 0 || ly < 0 || lx >= layer.width || ly >= layer.height) return [0, 0, 0, 0]
  return Array.from(layer.data.subarray((ly * layer.width + lx) * 4, (ly * layer.width + lx) * 4 + 4))
}

/** What shows at (x, y) with the layers drawn back to front over the skin, the upper one moved by `dx`. */
function shown(layers: RasterLayer[], x: number, y: number, dx = 0): number[] {
  let pixel = [240, 210, 200]
  for (const [index, layer] of layers.entries()) {
    const [r, g, b, a] = at(layer, x - (index === layers.length - 1 ? dx : 0), y)
    const k = a / 255
    pixel = [r * k + pixel[0] * (1 - k), g * k + pixel[1] * (1 - k), b * k + pixel[2] * (1 - k)]
  }
  return pixel.map(Math.round)
}

// Two locks side by side, cut on x = 200: the left one drawn first.
const DARK = [30, 30, 50] as const
const DARKER = [20, 20, 40] as const
const left = () => lock('front-hair-1', DARK, { left: 100, top: 0, width: 100, height: 80 })
const right = () => lock('front-hair-2', DARKER, { left: 200, top: 0, width: 100, height: 80 })

test('a lock reaches under the lock above it with that lock\'s pixels', () => {
  const [under, over] = underlapHairLocks([left(), right()])
  assert.deepEqual(at(under, 202, 40), [...DARKER, 255], 'the lock above\'s own colour, under it')
  assert.deepEqual(at(under, 260, 40), [0, 0, 0, 0], 'only near the cut')
  assert.deepEqual(at(over, 198, 40), [0, 0, 0, 0], 'the top lock is left as it was')
})

test('nothing at rest changes, and the opened cut shows hair, not skin', () => {
  const before = [left(), right()]
  const after = underlapHairLocks([left(), right()])
  for (const x of [195, 199, 200, 201, 205]) assert.deepEqual(shown(after, x, 40), shown(before, x, 40), `x ${x}`)
  // The lock above swings 2 px right: without the underlap the skin shows at x 200..201.
  assert.deepEqual(shown(before, 200, 40, 2), [240, 210, 200])
  assert.deepEqual(shown(after, 200, 40, 2), [...DARKER])
})

test('a single lock, or front hair that was not cut, is left alone', () => {
  const one = [left()]
  assert.equal(underlapHairLocks(one)[0], one[0])
  const whole = [lock('front-hair', DARK, { left: 100, top: 0, width: 100, height: 80 }), lock('face', DARKER, { left: 200, top: 0, width: 100, height: 80 }, 'face')]
  assert.deepEqual(underlapHairLocks(whole), whole)
})
