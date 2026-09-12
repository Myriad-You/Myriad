import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  isItemListCursor,
  itemListHasMore,
  itemListRequest,
} from './itemList.ts'

describe('item list cursor', () => {
  it('prefers next_cursor over item count', () => {
    assert.equal(itemListHasMore('1:2', 1, 20), true)
    assert.equal(itemListHasMore(null, 20, 20), false)
    assert.equal(itemListHasMore(undefined, 20, 20), true)
    assert.equal(itemListHasMore(undefined, 3, 20), false)
  })

  it('drops page when a cursor is present', () => {
    assert.equal(isItemListCursor('1700000000000:2'), true)
    assert.equal(isItemListCursor('x:2'), false)
    assert.equal(isItemListCursor('1:0'), false)
    assert.deepEqual(itemListRequest({ cursor: '1:2', page: 3, perPage: 20 }), {
      cursor: '1:2',
      per_page: 20,
    })
    assert.deepEqual(
      itemListRequest({ cursor: 'bad', page: 3, perPage: 20 }),
      {
        page: 3,
        per_page: 20,
      },
    )
    assert.deepEqual(itemListRequest({ page: 1, perPage: 20 }), {
      page: 1,
      per_page: 20,
    })
  })
})
