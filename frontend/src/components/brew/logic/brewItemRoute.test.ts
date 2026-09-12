import assert from 'node:assert/strict'
import { it } from 'node:test'
import {
  brewItemNavigateMode,
  brewItemParamId,
  brewOpenedItemState,
  restoreAfterFailedOpen,
  shouldLeaveFailedItemRoute,
  shouldPopOpenedItem,
} from './brewItemRoute'

it('pushes an own-item URL from the board and pops only that session open', () => {
  assert.equal(brewItemNavigateMode(undefined, '9'), 'push')
  assert.equal(brewItemNavigateMode('9', '9'), 'none')
  assert.equal(brewItemNavigateMode('9', '10'), 'push')
  assert.equal(brewItemNavigateMode('9', undefined), 'replace')
  assert.equal(shouldPopOpenedItem(brewOpenedItemState(9), 9), true)
  assert.equal(shouldPopOpenedItem(brewOpenedItemState(9), 8), false)
  assert.equal(shouldPopOpenedItem(undefined, 9), false)
})

it('leaves a failed deep link only while still on that item URL', () => {
  assert.equal(brewItemParamId('9'), 9)
  assert.equal(brewItemParamId('0'), null)
  assert.equal(brewItemParamId('x'), null)
  assert.equal(shouldLeaveFailedItemRoute('9', '9', undefined), true)
  assert.equal(shouldLeaveFailedItemRoute('9', '9', 9), false)
  assert.equal(shouldLeaveFailedItemRoute('9', '10', undefined), false)
  assert.equal(shouldLeaveFailedItemRoute('nope', 'nope', undefined), true)
  assert.deepEqual(
    restoreAfterFailedOpen('9', '9', undefined, { id: 3, own: true }),
    { path: '/brew/item/3', param: '3' },
  )
  assert.deepEqual(
    restoreAfterFailedOpen('9', '9', undefined, { id: 3, own: false }),
    { path: '/brew', param: undefined },
  )
  assert.equal(restoreAfterFailedOpen('9', '10', undefined, null), null)
})
