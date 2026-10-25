import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { formatWithoutErrorTable } from './formatUserFacingError'

describe('formatWithoutErrorTable', () => {
  it('keeps the caller fallback and adds a readable detail', () => {
    assert.equal(formatWithoutErrorTable(new Error('Comment is too long'), 'Send failed'), 'Send failed: Comment is too long')
  })

  it('drops noise and internal dumps, keeping only the fallback', () => {
    assert.equal(formatWithoutErrorTable(new TypeError('Failed to fetch'), 'Load failed'), 'Load failed')
    assert.equal(formatWithoutErrorTable(new Error('relation "users" does not exist'), 'Load failed'), 'Load failed')
  })

  it('still says something without a fallback', () => {
    assert.equal(formatWithoutErrorTable(new Error('Quota exceeded')), 'Quota exceeded')
    assert.equal(formatWithoutErrorTable('offline'), 'offline')
  })
})
