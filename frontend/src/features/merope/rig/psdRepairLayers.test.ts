import type { RecoveredPiece } from './psdRepairLayers'
import assert from 'node:assert/strict'
import test from 'node:test'
import { mergeByGroup } from './psdRepairLayers'

function piece(key: number, role: RecoveredPiece['role'], group: RecoveredPiece['group'] = 'head'): RecoveredPiece {
  return {
  pixels: new Map([[key, [255, 0, 0, 255] as const]]),
  above: key,
  neighbour: -1,
  owner: -1,
  group,
  role,
}
}

test('merged to fit the budget, recovered clips keep their role so they still turn with the head', () => {
  const merged = mergeByGroup([piece(1, 'headwear'), piece(2, 'headwear'), piece(3, 'headwear')], 1)
  assert.equal(merged.length, 1)
  assert.equal(merged[0].role, 'headwear')
  assert.equal(merged[0].pixels.size, 3)
  assert.equal(merged[0].above, 3)
})

test('when one layer per role is still over budget, a body part shares one plain rigid layer', () => {
  const merged = mergeByGroup([piece(1, 'headwear'), piece(2, 'earwear'), piece(3, 'objects', 'body')], 1)
  assert.equal(merged.length, 1)
  assert.equal(merged[0].role, 'objects')
  assert.equal(merged[0].group, 'head')
  assert.equal(merged[0].pixels.size, 2)
})
