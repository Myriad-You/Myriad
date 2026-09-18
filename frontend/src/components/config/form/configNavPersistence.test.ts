import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  CONFIG_NAV_DEFAULT_SECTION,
  CONFIG_NAV_SECTIONS,
  resolveConfigSectionFromSearch,
} from './configNavPersistence.ts'

describe('resolveConfigSectionFromSearch', () => {
  it('keeps AI when there is no persona subpage', () => {
    const params = new URLSearchParams('section=ai')
    assert.equal(resolveConfigSectionFromSearch(params, true), 'ai')
  })

  it('does not treat Agent as a /config room', () => {
    const merope = new URLSearchParams('section=ai&page=merope')
    const setup = new URLSearchParams('section=ai&page=merope-setup')
    const bare = new URLSearchParams('page=merope')
    const agent = new URLSearchParams('section=agent')
    assert.equal(resolveConfigSectionFromSearch(merope, true), 'ai')
    assert.equal(resolveConfigSectionFromSearch(setup, false), 'ai')
    assert.equal(resolveConfigSectionFromSearch(bare, true), null)
    assert.equal(resolveConfigSectionFromSearch(agent, true), null)
  })

  it('maps the old Laboratory deep link to lab', () => {
    const legacy = new URLSearchParams('section=tripo')
    const current = new URLSearchParams('section=lab')
    assert.equal(resolveConfigSectionFromSearch(legacy, true), 'lab')
    assert.equal(resolveConfigSectionFromSearch(current, false), 'lab')
  })

  it('rejects federation deep links when the gate is closed', () => {
    const params = new URLSearchParams('section=federation')
    assert.equal(resolveConfigSectionFromSearch(params, true), 'federation')
    assert.equal(resolveConfigSectionFromSearch(params, true, false), null)
    assert.equal(resolveConfigSectionFromSearch(params, false), null)
  })
})

describe('CONFIG_NAV_SECTIONS', () => {
  it('lists settings in the default sidebar order', () => {
    assert.deepEqual(
      [...CONFIG_NAV_SECTIONS],
      [
        'basic',
        'platforms',
        'ai',
        'notifications',
        'oauth',
        'users',
        'permissions',
        'federation',
        'modules',
        'advanced',
        'lab',
        'about',
      ],
    )
    assert.equal(CONFIG_NAV_DEFAULT_SECTION, CONFIG_NAV_SECTIONS[0])
  })
})
