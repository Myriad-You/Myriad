import type { RasterLayer } from './anime25dImportTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { raiseEarwearOverFrontHair } from './earwearOrder'

function layer(id: string, role: RasterLayer['role'], left: number, top: number, width: number, height: number): RasterLayer {
  const data = new Uint8ClampedArray(width * height * 4).fill(255)
  return { id, role, sourceName: id, order: 0, side: null, group: 'head', left, top, width, height, data }
}

test('an earring over the hair at rest goes over the front hair, so a turning lock passes behind it', () => {
  const back = layer('back', 'back-hair', 0, 0, 100, 100)
  const earring = layer('earring', 'earwear', 10, 60, 10, 20)
  const face = layer('face', 'face', 30, 0, 40, 60)
  const lock = layer('lock', 'front-hair', 30, 0, 40, 50)
  const output = raiseEarwearOverFrontHair([back, earring, face, lock])
  assert.deepEqual(output.map((l) => l.id), ['back', 'face', 'lock', 'earring'])
})

test('an earring a lock covers at rest keeps its place under it', () => {
  const back = layer('back', 'back-hair', 0, 0, 100, 100)
  const earring = layer('earring', 'earwear', 10, 60, 10, 20)
  const lock = layer('lock', 'front-hair', 5, 0, 20, 70)
  const output = raiseEarwearOverFrontHair([back, earring, lock])
  assert.deepEqual(output.map((l) => l.id), ['back', 'earring', 'lock'])
})
