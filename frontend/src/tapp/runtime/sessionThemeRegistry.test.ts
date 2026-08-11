import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  clearSessionThemes,
  listSessionThemes,
  registerSessionTheme,
  unregisterSessionTheme,
} from './sessionThemeRegistry'

describe('TAPP session theme registry', () => {
  it('scopes themes by owner and clears them on teardown', () => {
    const first = {}
    const second = {}
    registerSessionTheme(first, 'app.one', {
      id: 'calm',
      name: 'Calm',
      surface: 'glass',
    })
    registerSessionTheme(second, 'app.two', {
      id: 'bold',
      name: 'Bold',
      glow: 'primary',
    })

    assert.equal(listSessionThemes(first).length, 1)
    assert.equal(listSessionThemes().length, 2)
    assert.equal(unregisterSessionTheme(first, 'calm'), true)
    assert.equal(listSessionThemes(first).length, 0)

    clearSessionThemes(second)
    assert.equal(listSessionThemes().length, 0)
  })
})
