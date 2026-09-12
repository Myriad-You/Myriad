import assert from 'node:assert/strict'
import { it } from 'node:test'
import { BrewRevisionChain } from './brewRevisionChain'

it('advances only contiguous local confirmations without bridging external writes', () => {
  const revisions = new BrewRevisionChain()
  revisions.record(7, 0, 1)
  revisions.record(7, 1, 2)
  revisions.record(7, 3, 4)
  assert.equal(revisions.advance(7, 0), 2)
  assert.equal(revisions.advance(7, 3), 4)
  assert.equal(revisions.advance(8, 0), 0)
  assert.equal(revisions.advance(7, undefined), undefined)
  revisions.record(7, 2, 4)
  assert.equal(revisions.advance(7, 2), 2)
  assert.equal(new BrewRevisionChain().advance(7, 0), 0)
})
