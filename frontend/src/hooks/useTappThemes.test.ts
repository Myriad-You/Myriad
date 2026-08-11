import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { mergeTappThemes } from './useTappThemes'

describe('TAPP theme source merge', () => {
  it('lets session themes override persisted ids and removes them after teardown', () => {
    const persisted = [
      { id: 'calm', name: 'Saved Calm', surface: 'solid' as const },
    ]
    const session = [
      { id: 'calm', name: 'Session Calm', surface: 'glass' as const },
      { id: 'bold', name: 'Session Bold', glow: 'primary' as const },
    ]

    assert.deepEqual(mergeTappThemes(persisted, session), [
      session[0],
      session[1],
    ])
    assert.deepEqual(mergeTappThemes(persisted, []), persisted)
  })
})
