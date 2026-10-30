import type { RasterLayer } from './anime25dImportTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { paintAccessoriesFromReference } from './accessoryPaint'

const SIZE = 10

function solid(id: string, role: RasterLayer['role'], rgb: readonly [number, number, number], box: { left: number; top: number; width: number; height: number }): RasterLayer {
  const data = new Uint8ClampedArray(box.width * box.height * 4)
  for (let i = 0; i < data.length; i += 4) data.set([...rgb, 255], i)
  return { id, role, sourceName: id, order: 0, side: null, group: 'head', ...box, data }
}

/** The portrait: pale gold bars on pink hair, everywhere the same. */
function portrait() {
  const data = new Uint8ClampedArray(SIZE * SIZE * 4)
  for (let i = 0; i < data.length; i += 4) data.set([240, 200, 210, 255], i)
  return { width: SIZE, height: SIZE, data }
}

function rgbAt(layer: RasterLayer, x: number, y: number) {
  return Array.from(layer.data.slice((y * layer.width + x) * 4, (y * layer.width + x) * 4 + 3))
}

test('an earring the decomposer darkened takes the portrait colours where nothing covers it', () => {
  const earring = solid('earring', 'earwear', [60, 40, 40], { left: 2, top: 2, width: 6, height: 6 })
  // A lock of hair over its top half.
  const lock = solid('lock', 'front-hair', [250, 170, 190], { left: 0, top: 0, width: SIZE, height: 5 })
  const [painted, hair] = paintAccessoriesFromReference([earring, lock], () => true, portrait())
  assert.deepEqual(rgbAt(painted, 2, 5), [240, 200, 210])
  // Under the lock it keeps its own colour; the lock is untouched.
  assert.deepEqual(rgbAt(painted, 2, 0), [60, 40, 40])
  assert.equal(hair, lock)
})

test('a layer hidden at rest does not stop the paint, and other roles are left alone', () => {
  const choker = solid('choker', 'neckwear', [60, 40, 40], { left: 0, top: 0, width: SIZE, height: SIZE })
  const sweat = solid('sweat', 'speechless-sweat', [0, 0, 255], { left: 0, top: 0, width: SIZE, height: SIZE })
  const top = solid('top', 'topwear', [60, 40, 40], { left: 0, top: 0, width: 4, height: 4 })
  const [painted, , shirt] = paintAccessoriesFromReference([choker, sweat, top], (layer) => layer.role !== 'speechless-sweat', portrait())
  assert.deepEqual(rgbAt(painted, 6, 6), [240, 200, 210])
  assert.equal(shirt, top)
})
