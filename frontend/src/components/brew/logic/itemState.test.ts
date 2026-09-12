import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { makeItem } from './fixtures.ts'
import {
  appendUniqueById,
  dropItem,
  dropItems,
  dropStarredId,
} from './itemState.ts'

describe('drop / append', () => {
  it('按 id 去掉、按 id 去重追加', () => {
    const a = makeItem({ id: 1 })
    const b = makeItem({ id: 2 })
    assert.deepEqual(
      dropItem([a, b], 1).map((item) => item.id),
      [2],
    )
    assert.deepEqual(
      dropItems([a, b], new Set([2])).map((item) => item.id),
      [1],
    )
    assert.deepEqual(
      appendUniqueById([a], [a, b]).map((item) => item.id),
      [1, 2],
    )
  })
})

describe('dropStarredId', () => {
  it('只从选中集合去掉该 id', () => {
    const ids = new Set([1])
    assert.equal(dropStarredId(ids, 1).has(1), false)
    assert.equal(dropStarredId(ids, 2), ids)
  })
})
