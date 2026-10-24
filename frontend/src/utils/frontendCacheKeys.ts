import {
  SITE_METADATA_CACHE_KEY,
  SITE_METADATA_CACHE_TIME_KEY,
} from './siteMetadataKeys'

/** Prefs whitelist (not cache). */
export const PRESERVE_LOCAL_KEYS = new Set([
  'theme',
  'locale',
  'animation-preference',
  'config_favorites',
  'phantasiai_tts_settings',
  'phantasi-reader-settings',
  'myriad-anim-auto-want-high',
  'myriad_session_hint',
  'myriad:tapp-playground:sessions:v2',
  'browser_geo_denied_v1',
])

export const KNOWN_LOCAL_CACHE_KEYS = [
  // 当前的动画档位（hooks/useAnimationLevel.ts 每次加载都重写），给换页的内联脚本读。
  'myriad-perf-mode',
  'myriad_profile_display_cache',
  'myriad_profile_display_cache_time',
  'myriad_wallpaper_color_cache_v5',
  'wallpaperColorCache',
  SITE_METADATA_CACHE_KEY,
  SITE_METADATA_CACHE_TIME_KEY,
  'quote_cache',
  'quote_cache_time',
  'quote_cache_source',
  'quote_data_cache',
  'recent_activities_cache_v2',
  'library_stats_cache',
  'weather_data_cache',
  'browser_geo_location_v1',
] as const

export const LOCAL_CACHE_PREFIXES = [
  'weather_data_',
  'weather_time_',
  'geo_location_',
  'geo_location_time_',
] as const

const CACHE_KEY_HEURISTIC = /cache|_ttl|sw-cached|color_palette|playlist/i

export function shouldRemoveLocalCacheKey(key: string): boolean {
  if (PRESERVE_LOCAL_KEYS.has(key)) return false
  if ((KNOWN_LOCAL_CACHE_KEYS as readonly string[]).includes(key)) return true
  if (LOCAL_CACHE_PREFIXES.some((prefix) => key.startsWith(prefix))) return true
  if (CACHE_KEY_HEURISTIC.test(key)) return true
  if (key.endsWith('_time') && key.includes('meta')) return true
  return false
}
