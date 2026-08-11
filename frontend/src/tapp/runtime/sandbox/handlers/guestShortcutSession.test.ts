import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { GuestShortcutSession } from './guestShortcutSession'

describe('guest shortcut session', () => {
  it('lists only session registrations and clears them on teardown', () => {
    const session = new GuestShortcutSession()
    session.register({ id: 'toggle', keys: 'Mod+M', action: 'toggle' })

    assert.deepEqual(session.list(), [
      { id: 'toggle', keys: 'Mod+M', action: 'toggle' },
    ])

    session.unregister('toggle')
    assert.deepEqual(session.list(), [])

    session.register({ id: 'next', keys: 'Mod+N', action: 'next' })
    session.clear()
    assert.deepEqual(session.list(), [])
  })
})
