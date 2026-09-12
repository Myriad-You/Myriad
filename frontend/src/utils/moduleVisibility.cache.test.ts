/**
 * @vitest-environment node
 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  DEFAULT_MODULE_VISIBILITY_PREFERENCES,
  dispatchModuleVisibilityPreferencesUpdated,
  getCachedModuleVisibilityPreferences,
  normalizeModuleVisibilityPreferences,
} from './moduleVisibility'

describe('module visibility session cache', () => {
  it('dispatchModuleVisibilityPreferencesUpdated warms getCachedModuleVisibilityPreferences', () => {
    const next = normalizeModuleVisibilityPreferences({
      modules: {
        ...DEFAULT_MODULE_VISIBILITY_PREFERENCES.modules,
        tapp: 'authenticated',
      },
    })
    dispatchModuleVisibilityPreferencesUpdated(next)
    const cached = getCachedModuleVisibilityPreferences()
    assert.ok(cached)
    assert.equal(cached!.modules.tapp, 'authenticated')
  })

  it('later dispatch overwrites prior cache (no stale admin-only leak)', () => {
    dispatchModuleVisibilityPreferencesUpdated(
      normalizeModuleVisibilityPreferences({
        modules: {
          ...DEFAULT_MODULE_VISIBILITY_PREFERENCES.modules,
          library: 'admin',
        },
      }),
    )
    dispatchModuleVisibilityPreferencesUpdated(
      normalizeModuleVisibilityPreferences({
        modules: {
          ...DEFAULT_MODULE_VISIBILITY_PREFERENCES.modules,
          library: 'all',
        },
      }),
    )
    assert.equal(getCachedModuleVisibilityPreferences()?.modules.library, 'all')
  })
})
