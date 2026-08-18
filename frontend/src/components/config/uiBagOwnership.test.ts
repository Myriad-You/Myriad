/**
 * Soft-reload contract for config save classifiers.
 * @vitest-environment node
 */
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  ALL_OWNED_UI_BAG_KEYS,
  bagFieldValue,
  configChangesNeedHardReload,
  configChangesNeedMetadataReload,
  configChangesNeedPlatformsCacheInvalidation,
  configChangesNeedPwaReload,
  configChangesNeedRuntimeReload,
  configChangesNeedWallpaperReload,
  RUNTIME_RELOAD_UI_BAG_KEYS,
} from './uiBagOwnership'

function deepEqual(a: unknown, b: unknown) {
  return JSON.stringify(a) === JSON.stringify(b)
}

function cfg(fields: Array<{ key: string; value: string }>) {
  return {
    platforms: [{ name: 'GitHub' }],
    auto_fetch: { enabled: false, interval_hours: 24 },
    ai_config: { enabled: true },
    ui_config: { config_fields: fields },
  }
}

describe('uiBagOwnership', () => {
  it('lists owned bag keys without base_url', () => {
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('wallpaper_url'))
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('analytics_enabled'))
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('music_enabled'))
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('agent_life_enabled'))
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('proxy_url'))
    assert.ok(!ALL_OWNED_UI_BAG_KEYS.includes('base_url'))
  })

  it('does not hard-reload pure UI bag edits', () => {
    const prev = cfg([
      { key: 'site_title', value: 'A' },
      { key: 'wallpaper_url', value: 'https://a' },
    ])
    const next = cfg([
      { key: 'site_title', value: 'B' },
      { key: 'wallpaper_url', value: 'https://b' },
    ])
    assert.equal(configChangesNeedHardReload(next, prev, deepEqual), false)
  })

  it('soft-reloads site metadata / SEO / GA without hard reload', () => {
    const prev = cfg([
      { key: 'site_title', value: 'A' },
      { key: 'site_keywords', value: '' },
      { key: 'site_noindex', value: 'false' },
      { key: 'ga_measurement_id', value: '' },
    ])
    const next = cfg([
      { key: 'site_title', value: 'B' },
      { key: 'site_keywords', value: 'blog' },
      { key: 'site_noindex', value: 'true' },
      { key: 'ga_measurement_id', value: 'G-TEST123' },
    ])
    assert.equal(configChangesNeedMetadataReload(next, prev), true)
    assert.equal(configChangesNeedMetadataReload(prev, prev), false)
    assert.equal(configChangesNeedHardReload(next, prev, deepEqual), false)
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('site_keywords'))
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('site_og_image'))
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('site_noindex'))
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('ga_measurement_id'))
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('umami_website_id'))
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('umami_script_url'))
  })

  it('soft-reloads pwa_enabled without hard reload', () => {
    const prev = cfg([{ key: 'pwa_enabled', value: 'true' }])
    const next = cfg([{ key: 'pwa_enabled', value: 'false' }])
    assert.equal(configChangesNeedPwaReload(next, prev), true)
    assert.equal(configChangesNeedPwaReload(prev, prev), false)
    assert.equal(configChangesNeedHardReload(next, prev, deepEqual), false)
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('pwa_enabled'))
  })

  it('soft-reloads wallpaper / evocative without hard reload', () => {
    const prev = cfg([
      { key: 'evocative_parallax', value: 'true' },
      { key: 'wallpaper_blur', value: '3' },
    ])
    const next = cfg([
      { key: 'evocative_parallax', value: 'false' },
      { key: 'wallpaper_blur', value: '3' },
    ])
    assert.equal(configChangesNeedHardReload(next, prev, deepEqual), false)
    assert.equal(configChangesNeedWallpaperReload(next, prev), true)
    assert.equal(configChangesNeedWallpaperReload(prev, prev), false)
    assert.equal(configChangesNeedRuntimeReload(next, prev), false)
  })

  it('does not hard-reload AI / platforms / auto_fetch', () => {
    const prev = cfg([{ key: 'proxy_url', value: '' }])

    const nextPlatforms = {
      ...prev,
      platforms: [{ name: 'Steam' }],
    }
    assert.equal(
      configChangesNeedHardReload(nextPlatforms, prev, deepEqual),
      false,
    )
    assert.equal(
      configChangesNeedPlatformsCacheInvalidation(
        nextPlatforms,
        prev,
        deepEqual,
      ),
      true,
    )
    assert.equal(configChangesNeedRuntimeReload(nextPlatforms, prev), false)

    const nextAi = {
      ...prev,
      ai_config: { enabled: false },
    }
    assert.equal(configChangesNeedHardReload(nextAi, prev, deepEqual), false)
    assert.equal(configChangesNeedRuntimeReload(nextAi, prev), false)
    assert.equal(
      configChangesNeedPlatformsCacheInvalidation(nextAi, prev, deepEqual),
      false,
    )

    const nextAutoFetch = {
      ...prev,
      auto_fetch: { enabled: true, interval_hours: 12 },
    }
    assert.equal(
      configChangesNeedHardReload(nextAutoFetch, prev, deepEqual),
      false,
    )
    assert.equal(configChangesNeedRuntimeReload(nextAutoFetch, prev), false)
  })

  it('classifies proxy / API mirror as runtime reload without hard reload', () => {
    const prev = cfg([{ key: 'proxy_url', value: '' }])
    const nextProxy = cfg([
      { key: 'proxy_url', value: 'http://127.0.0.1:7890' },
    ])
    assert.equal(configChangesNeedHardReload(nextProxy, prev, deepEqual), false)
    assert.equal(configChangesNeedRuntimeReload(nextProxy, prev), true)
    assert.equal(configChangesNeedRuntimeReload(prev, prev), false)

    const nextMirror = cfg([
      { key: 'gemini_base_url', value: 'https://mirror.example/gemini' },
      { key: 'github_api_base_url', value: 'https://mirror.example/github' },
    ])
    const prevMirror = cfg([
      { key: 'gemini_base_url', value: '' },
      { key: 'github_api_base_url', value: '' },
    ])
    assert.equal(
      configChangesNeedHardReload(nextMirror, prevMirror, deepEqual),
      false,
    )
    assert.equal(configChangesNeedRuntimeReload(nextMirror, prevMirror), true)

    for (const key of [
      'memory_saver_enabled',
      'agent_life_enabled',
      'proxy_enabled',
      'proxy_url',
      'proxy_bypass',
      'gemini_base_url',
      'github_api_base_url',
    ] as const) {
      assert.ok(
        RUNTIME_RELOAD_UI_BAG_KEYS.includes(key),
        `RUNTIME_RELOAD_UI_BAG_KEYS should include ${key}`,
      )
    }
  })

  it('bagFieldValue reads by key', () => {
    assert.equal(
      bagFieldValue([{ key: 'a', value: '1' }, { key: 'b', value: '2' }], 'b'),
      '2',
    )
    assert.ok(RUNTIME_RELOAD_UI_BAG_KEYS.length > 0)
  })
})
