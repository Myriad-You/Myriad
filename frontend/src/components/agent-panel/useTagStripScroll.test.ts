import assert from 'node:assert/strict'
import { test } from 'node:test'
import { tagStripOverflow } from './useTagStripScroll'

test('tag strip overflow is empty when everything fits', () => {
  assert.equal(tagStripOverflow(0, 120, 200), '')
  assert.equal(tagStripOverflow(0, 200, 200), '')
})

test('tag strip overflow marks the end, start, or both', () => {
  assert.equal(tagStripOverflow(0, 400, 200), 'end')
  assert.equal(tagStripOverflow(200, 400, 200), 'start')
  assert.equal(tagStripOverflow(80, 400, 200), 'both')
})
