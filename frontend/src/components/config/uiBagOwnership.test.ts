/** @vitest-environment node */
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  ADVANCED_RESET_KEYS,
  AGENT_UI_RESET_KEYS,
  ALL_OWNED_UI_BAG_KEYS,
  bagFieldValue,
  configChangesNeedIslandReload,
  configChangesNeedMetadataReload,
  configChangesNeedPersonaPublicNameRefresh,
  configChangesNeedPlatformsCacheInvalidation,
  configChangesNeedPwaReload,
  configChangesNeedRuntimeReload,
  configChangesNeedSpeechPipelineReload,
  configChangesNeedWallpaperReload,
  PERSONA_PUBLIC_NAME_UI_BAG_KEYS,
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
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('island_show_greeting'))
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('island_show_tapp'))
    assert.ok(!ALL_OWNED_UI_BAG_KEYS.includes('merope_enabled'))
    assert.ok(!ALL_OWNED_UI_BAG_KEYS.includes('merope_speech_enabled'))
    assert.ok(!ADVANCED_RESET_KEYS.includes('merope_enabled'))
    assert.ok(!ADVANCED_RESET_KEYS.includes('merope_speech_enabled'))
    assert.ok(AGENT_UI_RESET_KEYS.includes('merope_enabled'))
    assert.ok(AGENT_UI_RESET_KEYS.includes('merope_speech_enabled'))
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('proxy_url'))
    assert.ok(!ALL_OWNED_UI_BAG_KEYS.includes('base_url'))
  })

  it('keeps pure UI bag edits out of backend runtime reload', () => {
    const prev = cfg([
      { key: 'site_title', value: 'A' },
      { key: 'wallpaper_url', value: 'https://a' },
    ])
    const next = cfg([
      { key: 'site_title', value: 'B' },
      { key: 'wallpaper_url', value: 'https://b' },
    ])
    assert.equal(configChangesNeedRuntimeReload(next, prev), false)
  })

  it('soft-reloads site metadata / SEO / GA with targeted refresh', () => {
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
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('site_keywords'))
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('site_seo_review_cadence'))
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('site_og_image'))
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('google_site_verification'))
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('site_noindex'))
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('ga_measurement_id'))
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('umami_website_id'))
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('umami_script_url'))
  })

  it('soft-reloads island content flags with targeted refresh', () => {
    const prev = cfg([{ key: 'island_show_greeting', value: 'true' }])
    const next = cfg([{ key: 'island_show_greeting', value: 'false' }])
    assert.equal(configChangesNeedIslandReload(next, prev), true)
    assert.equal(configChangesNeedIslandReload(prev, prev), false)
  })

  it('soft-reloads pwa_enabled with targeted refresh', () => {
    const prev = cfg([{ key: 'pwa_enabled', value: 'true' }])
    const next = cfg([{ key: 'pwa_enabled', value: 'false' }])
    assert.equal(configChangesNeedPwaReload(next, prev), true)
    assert.equal(configChangesNeedPwaReload(prev, prev), false)
    assert.ok(ALL_OWNED_UI_BAG_KEYS.includes('pwa_enabled'))
  })

  it('soft-reloads wallpaper / evocative with targeted refresh', () => {
    const prev = cfg([
      { key: 'evocative_parallax', value: 'true' },
      { key: 'wallpaper_blur', value: '3' },
    ])
    const next = cfg([
      { key: 'evocative_parallax', value: 'false' },
      { key: 'wallpaper_blur', value: '3' },
    ])
    assert.equal(configChangesNeedWallpaperReload(next, prev), true)
    assert.equal(configChangesNeedWallpaperReload(prev, prev), false)
    assert.equal(configChangesNeedRuntimeReload(next, prev), false)
  })

  it('classifies AI / platforms / auto_fetch refresh needs', () => {
    const prev = cfg([{ key: 'proxy_url', value: '' }])

    const nextPlatforms = {
      ...prev,
      platforms: [{ name: 'Steam' }],
    }
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
    assert.equal(configChangesNeedRuntimeReload(nextAi, prev), false)
    assert.equal(
      configChangesNeedPlatformsCacheInvalidation(nextAi, prev, deepEqual),
      false,
    )

    const nextAutoFetch = {
      ...prev,
      auto_fetch: { enabled: true, interval_hours: 12 },
    }
    assert.equal(configChangesNeedRuntimeReload(nextAutoFetch, prev), false)
  })

  it('classifies proxy / API mirror as runtime reload with targeted refresh', () => {
    const prev = cfg([{ key: 'proxy_url', value: '' }])
    const nextProxy = cfg([
      { key: 'proxy_url', value: 'http://127.0.0.1:7890' },
    ])
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
    assert.equal(configChangesNeedRuntimeReload(nextMirror, prevMirror), true)
    assert.equal(
      configChangesNeedSpeechPipelineReload(nextMirror, prevMirror),
      false,
    )
    assert.equal(
      configChangesNeedSpeechPipelineReload(
        cfg([{ key: 'merope_speech_enabled', value: 'true' }]),
        cfg([{ key: 'merope_speech_enabled', value: 'false' }]),
      ),
      true,
    )

    for (const key of [
      'memory_saver_enabled',
      'precise_location_enabled',
      'merope_enabled',
      'merope_speech_enabled',
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

  it('refreshes the public persona name only when merope_enabled changes', () => {
    assert.deepEqual(Iterator.from(PERSONA_PUBLIC_NAME_UI_BAG_KEYS).toArray(), [
      'merope_enabled',
    ])
    assert.equal(
      configChangesNeedPersonaPublicNameRefresh(
        cfg([{ key: 'merope_enabled', value: 'false' }]),
        cfg([{ key: 'merope_enabled', value: 'true' }]),
      ),
      true,
    )
    assert.equal(
      configChangesNeedPersonaPublicNameRefresh(
        cfg([{ key: 'merope_speech_enabled', value: 'false' }]),
        cfg([{ key: 'merope_speech_enabled', value: 'true' }]),
      ),
      false,
    )
    assert.equal(
      configChangesNeedPersonaPublicNameRefresh(
        cfg([{ key: 'merope_enabled', value: 'true' }]),
        cfg([{ key: 'merope_enabled', value: 'true' }]),
      ),
      false,
    )
  })

  it('bagFieldValue reads by key', () => {
    assert.equal(
      bagFieldValue(
        [
          { key: 'a', value: '1' },
          { key: 'b', value: '2' },
        ],
        'b',
      ),
      '2',
    )
    assert.ok(RUNTIME_RELOAD_UI_BAG_KEYS.length > 0)
  })
})
