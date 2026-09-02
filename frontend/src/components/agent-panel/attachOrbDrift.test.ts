import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { attachOrbDriftSpeed } from './attachOrbDrift'

describe('attachOrbDriftSpeed', () => {
  it('moves faster while working than while thinking', () => {
    assert.ok(attachOrbDriftSpeed('working') > attachOrbDriftSpeed('thinking'))
  })
})
