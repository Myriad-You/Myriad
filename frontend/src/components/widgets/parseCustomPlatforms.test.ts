/**
 * Pure-function tests for custom platform JSON normalization.
 * Run from frontend/:
 *   node --experimental-strip-types --test src/components/widgets/parseCustomPlatforms.test.ts
 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { parseCustomPlatforms } from './parseCustomPlatforms.ts'

describe('parseCustomPlatforms', () => {
  it('returns [] for null, undefined, and empty string', () => {
    assert.deepEqual(parseCustomPlatforms(null), [])
    assert.deepEqual(parseCustomPlatforms(undefined), [])
    assert.deepEqual(parseCustomPlatforms(''), [])
  })

  it('returns [] when the stored string is JSON null', () => {
    assert.deepEqual(parseCustomPlatforms('null'), [])
  })

  it('returns [] for a non-array JSON value', () => {
    assert.deepEqual(parseCustomPlatforms('{}'), [])
    assert.deepEqual(parseCustomPlatforms({ id: 'x' }), [])
  })

  it('returns [] for invalid JSON', () => {
    assert.deepEqual(parseCustomPlatforms('not-json'), [])
  })

  it('parses a JSON array string', () => {
    const raw = JSON.stringify([{ id: 'custom_1', name: 'Discord' }])
    assert.deepEqual(parseCustomPlatforms(raw), [
      { id: 'custom_1', name: 'Discord' },
    ])
  })

  it('passes through an already-parsed array', () => {
    const platforms = [{ id: 'custom_1', name: 'Discord' }]
    assert.deepEqual(parseCustomPlatforms(platforms), platforms)
  })
})
