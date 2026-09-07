import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { addDoneId, parseDoneIds } from './tourDone'

describe('parseDoneIds', () => {
  it('reads a string array', () => {
    assert.deepEqual(parseDoneIds('["home-owner"]'), ['home-owner'])
  })

  it('returns empty on junk', () => {
    assert.deepEqual(parseDoneIds('{'), [])
    assert.deepEqual(parseDoneIds('null'), [])
    assert.deepEqual(parseDoneIds(null), [])
  })
})

describe('addDoneId', () => {
  it('appends once', () => {
    assert.deepEqual(addDoneId([], 'home-visitor'), ['home-visitor'])
    assert.deepEqual(addDoneId(['home-visitor'], 'home-visitor'), [
      'home-visitor',
    ])
  })
})
