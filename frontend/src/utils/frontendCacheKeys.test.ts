import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { shouldRemoveLocalCacheKey } from './frontendCacheKeys.ts'

describe('shouldRemoveLocalCacheKey', () => {
  it('preserves preference and work keys', () => {
    for (const key of [
      'theme',
      'locale',
      'animation-preference',
      'config_favorites',
      'brewlia_tts_settings',
      'brew-reader-settings',
      'myriad-anim-auto-want-high',
      'myriad_session_hint',
      'myriad:tapp-playground:sessions:v2',
      'browser_geo_denied_v1',
    ]) {
      assert.equal(shouldRemoveLocalCacheKey(key), false, key)
    }
  })

  it('removes known and heuristic cache keys', () => {
    for (const key of [
      'myriad_profile_display_cache',
      'wallpaperColorCache',
      'recent_activities_cache_v2',
      'weather_data_cache',
      'weather_data_beijing',
      'geo_location_1.2.3.4',
      'some_feature_cache_v1',
      'site_metadata_time',
    ]) {
      assert.equal(shouldRemoveLocalCacheKey(key), true, key)
    }
  })

  it('does not remove unrelated non-cache keys', () => {
    assert.equal(shouldRemoveLocalCacheKey('unrelated_user_flag'), false)
  })
})
