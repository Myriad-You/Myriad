import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { wallpaperUnknownMessage } from './wallpaperError.ts'

const copy = {
  unsafeUrl: 'unsafe',
  imageLoadFailed: 'load-failed',
  unknown: 'unknown',
}

describe('wallpaperUnknownMessage', () => {
  it('keeps a useful Error message and drops API Error: status', () => {
    assert.equal(
      wallpaperUnknownMessage(new Error('timeout while decoding'), copy),
      'timeout while decoding',
    )
    assert.equal(
      wallpaperUnknownMessage(new Error('API Error: 502'), copy),
      'unknown',
    )
    assert.equal(wallpaperUnknownMessage('nope', copy), 'unknown')
  })
})
