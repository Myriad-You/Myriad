import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { sanitizeStorageValue, validateStorageKey } from './security.ts'

describe('validateStorageKey', () => {
  it('accepts the sandbox charset and rejects path tricks', () => {
    assert.equal(validateStorageKey('user.preferences').valid, true)
    assert.equal(validateStorageKey('a:b_c-d.e').valid, true)
    assert.equal(validateStorageKey('').valid, false)
    assert.equal(validateStorageKey('../x').valid, false)
    assert.equal(validateStorageKey('.hidden').valid, false)
    assert.equal(validateStorageKey('trail.').valid, false)
    assert.equal(validateStorageKey('slash/key').valid, false)
    assert.equal(validateStorageKey('a'.repeat(257)).valid, false)
  })

  it('does not enforce host-reserved prefixes; that gate is the backend key validator', () => {
    assert.equal(validateStorageKey('_private.token').valid, true)
    assert.equal(validateStorageKey('_shared.posts').valid, true)
    assert.equal(validateStorageKey('_settings.theme').valid, true)
  })
})

describe('sanitizeStorageValue', () => {
  it('keeps JSON scalars and objects, and returns undefined unchanged', () => {
    assert.equal(sanitizeStorageValue(null), null)
    assert.equal(sanitizeStorageValue(undefined), undefined)
    assert.equal(sanitizeStorageValue(1), 1)
    assert.equal(sanitizeStorageValue(false), false)
    assert.equal(sanitizeStorageValue('ok'), 'ok')
    assert.deepEqual(sanitizeStorageValue({ a: 1 }), { a: 1 })
  })

  it('rejects oversized strings', () => {
    assert.throws(
      () => sanitizeStorageValue('x'.repeat(1024 * 1024 + 1)),
      /too large/i,
    )
  })
})
