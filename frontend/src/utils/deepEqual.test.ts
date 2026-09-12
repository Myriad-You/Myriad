import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { deepEqual } from './deepEqual.ts'

describe('deepEqual', () => {
  it('compares primitives and null', () => {
    assert.equal(deepEqual(1, 1), true)
    assert.equal(deepEqual('a', 'a'), true)
    assert.equal(deepEqual(null, null), true)
    assert.equal(deepEqual(null, undefined), false)
    assert.equal(deepEqual(1, 2), false)
  })

  it('compares arrays by order', () => {
    assert.equal(deepEqual([1, 2], [1, 2]), true)
    assert.equal(deepEqual([1, 2], [2, 1]), false)
  })

  it('compares objects independent of key order', () => {
    assert.equal(deepEqual({ a: 1, b: 2 }, { b: 2, a: 1 }), true)
    assert.equal(deepEqual({ a: 1 }, { a: 2 }), false)
    assert.equal(deepEqual({ a: { c: 1 } }, { a: { c: 1 } }), true)
  })
})
