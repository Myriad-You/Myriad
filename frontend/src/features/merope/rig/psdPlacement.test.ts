import assert from 'node:assert/strict'
import test from 'node:test'
import { psdPlacement } from './psdPlacement'

test('a square PSD is the portrait padded to a square, as before', () => {
  // A 3:4 portrait on the demo's 1280 square.
  const placement = psdPlacement({ width: 1086, height: 1448 }, 1280, 1280)
  const scale = 1280 / 1448
  assert.equal(placement.x, 181 * scale)
  assert.equal(placement.y, 0)
  assert.equal(placement.width, 1086 * scale)
  assert.equal(placement.height, 1280)
  assert.deepEqual(placement.map(10, 20), [(10 + 181) * scale, 20 * scale])
})

test('a fitted canvas places the portrait as See-through does', () => {
  // fit_placement((1448, 1086), (1664, 1088)): scale 1.00184, 1088×1451 at (0, 106).
  const placement = psdPlacement({ width: 1086, height: 1448 }, 1088, 1664)
  assert.deepEqual(
    [placement.x, placement.y, placement.width, placement.height],
    [0, 106, 1088, 1451],
  )
  const [x, y] = placement.map(1086, 1448)
  assert.ok(Math.abs(x - 1088) < 1e-9 && Math.abs(y - (106 + 1451)) < 1e-9)
  // A tall 9:16 full figure fills the height and is centred across.
  const tall = psdPlacement({ width: 864, height: 1536 }, 1088, 1664)
  assert.deepEqual([tall.x, tall.y, tall.width, tall.height], [76, 0, 936, 1664])
})
