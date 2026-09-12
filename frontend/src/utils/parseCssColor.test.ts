import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { parseCssColor } from './readableColor.ts'

describe('parseCssColor', () => {
  it('parses 6-digit hex', () => {
    assert.deepEqual(parseCssColor('#94a3b8'), { r: 148, g: 163, b: 184 })
  })

  it('parses shorthand and 8-digit hex, dropping alpha', () => {
    assert.deepEqual(parseCssColor('#abc'), { r: 170, g: 187, b: 204 })
    assert.deepEqual(parseCssColor('#94a3b880'), { r: 148, g: 163, b: 184 })
  })

  it('parses comma-separated rgb()', () => {
    assert.deepEqual(parseCssColor('rgb(107, 114, 128)'), {
      r: 107,
      g: 114,
      b: 128,
    })
  })

  it('parses space-separated rgb() with slash alpha', () => {
    assert.deepEqual(parseCssColor('rgb(107 114 128 / 0.5)'), {
      r: 107,
      g: 114,
      b: 128,
    })
  })

  it('parses rgba() and ignores the alpha channel', () => {
    assert.deepEqual(parseCssColor('rgba(1, 2, 3, 0.25)'), { r: 1, g: 2, b: 3 })
  })

  it('parses percentage channels', () => {
    assert.deepEqual(parseCssColor('rgb(100%, 0%, 50%)'), {
      r: 255,
      g: 0,
      b: 128,
    })
  })

  it('clamps out-of-range channels', () => {
    assert.deepEqual(parseCssColor('rgb(300, -20, 128)'), {
      r: 255,
      g: 0,
      b: 128,
    })
  })

  it('returns null for empty, malformed, or unsupported values', () => {
    assert.equal(parseCssColor(''), null)
    assert.equal(parseCssColor(null), null)
    assert.equal(parseCssColor(undefined), null)
    assert.equal(parseCssColor('not-a-color'), null)
    assert.equal(parseCssColor('rgb(1, 2)'), null)
    assert.equal(parseCssColor('rgb()'), null)
    assert.equal(parseCssColor('#12345'), null)
  })
})
