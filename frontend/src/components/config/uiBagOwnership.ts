/** bag-key ownership; section re-exports must not drift */

/** UI bag; base_url is SiteUrlField */
export const UI_RESET_KEYS: readonly string[] = Object.freeze([
  'wallpaper_url',
  'wallpaper_blur',
  'site_title',
  'site_description',
  'site_favicon',
  'site_keywords',
  'site_og_image',
  'google_site_verification',
  'site_noindex',
  'site_visibility_policy',
  'site_ai_intro',
  'site_seo_review_cadence',
  'pwa_enabled',
  'site_icp',
  'site_gongan',
  'cloud_sponsors',
  'site_footer_custom',
  'evocative_parallax',
  'evocative_dynamic_blur',
  'evocative_ripple',
  'evocative_fps',
  'evocative_ripple_quality',
])

export const METADATA_SOFT_RELOAD_UI_BAG_KEYS: readonly string[] =
  Object.freeze([
    'site_title',
    'site_description',
    'site_favicon',
    'site_keywords',
    'site_og_image',
    'google_site_verification',
    'site_noindex',
    'site_visibility_policy',
    'site_ai_intro',
    'ga_measurement_id',
    'umami_website_id',
    'umami_script_url',
  ])

export const FOOTER_SOFT_RELOAD_UI_BAG_KEYS: readonly string[] = Object.freeze([
  'site_icp',
  'site_gongan',
  'cloud_sponsors',
  'site_footer_custom',
])

export const PLATFORMS_UI_RESET_KEYS: readonly string[] = Object.freeze([
  'analytics_enabled',
  'ga_measurement_id',
  'umami_website_id',
  'umami_script_url',
])

/** library/report/hitokoto are side drafts, not this bag */
export const MODULE_UI_RESET_KEYS: readonly string[] = Object.freeze([
  'music_enabled',
  'music_source',
  'music_playlist_id',
  'island_show_greeting',
  'island_show_weather',
  'island_show_quote',
  'island_show_music',
  'island_show_tapp',
])

export const ADVANCED_RESET_KEYS: readonly string[] = Object.freeze([
  'memory_saver_enabled',
  'precise_location_enabled',
  'proxy_enabled',
  'proxy_url',
  'proxy_bypass',
  'gemini_base_url',
  'github_api_base_url',
])

export const AGENT_UI_RESET_KEYS: readonly string[] = Object.freeze([
  'merope_enabled',
  'merope_speech_enabled',
])

/** UI bag; base_url is SiteUrlField */
export const ALL_OWNED_UI_BAG_KEYS: readonly string[] = Object.freeze([
  ...UI_RESET_KEYS,
  ...PLATFORMS_UI_RESET_KEYS,
  ...MODULE_UI_RESET_KEYS,
  ...ADVANCED_RESET_KEYS,
])

/** these bag keys hot-reload; no location.reload */
export const RUNTIME_RELOAD_UI_BAG_KEYS: readonly string[] = Object.freeze([
  ...ADVANCED_RESET_KEYS,
  ...AGENT_UI_RESET_KEYS,
])

export const WALLPAPER_SOFT_RELOAD_UI_BAG_KEYS: readonly string[] =
  Object.freeze([
    'wallpaper_url',
    'wallpaper_blur',
    'evocative_parallax',
    'evocative_dynamic_blur',
    'evocative_ripple',
    'evocative_fps',
    'evocative_ripple_quality',
  ])

export function bagFieldValue(
  fields: Array<{ key: string; value: string }> | undefined,
  key: string,
): string | undefined {
  return fields?.find((f) => f.key === key)?.value
}

function bagKeysChanged(
  nextFields: Array<{ key: string; value: string }> | undefined,
  prevFields: Array<{ key: string; value: string }> | undefined,
  keys: readonly string[],
): boolean {
  for (const key of keys) {
    if (bagFieldValue(nextFields, key) !== bagFieldValue(prevFields, key)) {
      return true
    }
  }
  return false
}

export function configChangesNeedRuntimeReload(
  next: {
    ui_config?: { config_fields?: Array<{ key: string; value: string }> }
  },
  prev: {
    ui_config?: { config_fields?: Array<{ key: string; value: string }> }
  },
): boolean {
  return bagKeysChanged(
    next.ui_config?.config_fields,
    prev.ui_config?.config_fields,
    RUNTIME_RELOAD_UI_BAG_KEYS,
  )
}

export const PERSONA_PUBLIC_NAME_UI_BAG_KEYS: readonly string[] = Object.freeze(
  ['merope_enabled'],
)

export function configChangesNeedPersonaPublicNameRefresh(
  next: {
    ui_config?: { config_fields?: Array<{ key: string; value: string }> }
  },
  prev: {
    ui_config?: { config_fields?: Array<{ key: string; value: string }> }
  },
): boolean {
  return bagKeysChanged(
    next.ui_config?.config_fields,
    prev.ui_config?.config_fields,
    PERSONA_PUBLIC_NAME_UI_BAG_KEYS,
  )
}

export function configChangesNeedSpeechPipelineReload(
  next: {
    ui_config?: { config_fields?: Array<{ key: string; value: string }> }
  },
  prev: {
    ui_config?: { config_fields?: Array<{ key: string; value: string }> }
  },
): boolean {
  return bagKeysChanged(
    next.ui_config?.config_fields,
    prev.ui_config?.config_fields,
    AGENT_UI_RESET_KEYS,
  )
}

/** platform save: invalidate library caches */
export function configChangesNeedPlatformsCacheInvalidation(
  next: { platforms: unknown },
  prev: { platforms: unknown },
  deepEqual: (a: unknown, b: unknown) => boolean,
): boolean {
  return !deepEqual(next.platforms, prev.platforms)
}

/** wallpaper fields: soft wallpaper refresh */
export function configChangesNeedWallpaperReload(
  next: {
    ui_config?: { config_fields?: Array<{ key: string; value: string }> }
  },
  prev: {
    ui_config?: { config_fields?: Array<{ key: string; value: string }> }
  },
): boolean {
  return bagKeysChanged(
    next.ui_config?.config_fields,
    prev.ui_config?.config_fields,
    WALLPAPER_SOFT_RELOAD_UI_BAG_KEYS,
  )
}

export const ISLAND_SOFT_RELOAD_UI_BAG_KEYS: readonly string[] = Object.freeze([
  'island_show_greeting',
  'island_show_weather',
  'island_show_quote',
  'island_show_music',
  'island_show_tapp',
])

export function configChangesNeedIslandReload(
  next: {
    ui_config?: { config_fields?: Array<{ key: string; value: string }> }
  },
  prev: {
    ui_config?: { config_fields?: Array<{ key: string; value: string }> }
  },
): boolean {
  return bagKeysChanged(
    next.ui_config?.config_fields,
    prev.ui_config?.config_fields,
    ISLAND_SOFT_RELOAD_UI_BAG_KEYS,
  )
}

export function configChangesNeedFooterReload(
  next: {
    ui_config?: { config_fields?: Array<{ key: string; value: string }> }
  },
  prev: {
    ui_config?: { config_fields?: Array<{ key: string; value: string }> }
  },
): boolean {
  return bagKeysChanged(
    next.ui_config?.config_fields,
    prev.ui_config?.config_fields,
    FOOTER_SOFT_RELOAD_UI_BAG_KEYS,
  )
}

/** site identity: soft document meta refresh */
export function configChangesNeedMetadataReload(
  next: {
    ui_config?: { config_fields?: Array<{ key: string; value: string }> }
  },
  prev: {
    ui_config?: { config_fields?: Array<{ key: string; value: string }> }
  },
): boolean {
  return bagKeysChanged(
    next.ui_config?.config_fields,
    prev.ui_config?.config_fields,
    METADATA_SOFT_RELOAD_UI_BAG_KEYS,
  )
}

/** PWA: register/unregister SW */
export const PWA_SOFT_RELOAD_UI_BAG_KEYS: readonly string[] = Object.freeze([
  'pwa_enabled',
])

export function configChangesNeedPwaReload(
  next: {
    ui_config?: { config_fields?: Array<{ key: string; value: string }> }
  },
  prev: {
    ui_config?: { config_fields?: Array<{ key: string; value: string }> }
  },
): boolean {
  return bagKeysChanged(
    next.ui_config?.config_fields,
    prev.ui_config?.config_fields,
    PWA_SOFT_RELOAD_UI_BAG_KEYS,
  )
}
