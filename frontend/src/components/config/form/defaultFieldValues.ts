import type { ConfigField } from './types'

export function defaultAiFieldValue(key: string): string {
  if (key === 'model') return 'gemini-3.6-flash'
  if (key === 'ai_image_provider') return 'openrouter'
  if (key === 'ai_image_model') return 'openai/gpt-image-2'
  if (key === 'ai_image_openai_base_url') return 'https://api.openai.com/v1'
  if (key === 'ai_image_volcengine_base_url')
    return 'https://ark.cn-beijing.volces.com/api/v3'
  if (key === 'lite_enabled') return 'false'
  if (key === 'lite_provider') return 'openai'
  if (key === 'lite_openai_model') return 'openai/gpt-oss-20b:free'
  if (key === 'lite_openai_base_url') return 'https://openrouter.ai/api/v1'
  if (key === 'lite_gemini_model') return 'gemini-3.5-flash-lite'
  if (key === 'pro_enabled') return 'false'
  return ''
}

export function defaultTripoFieldValue(key: string): string {
  if (key === 'tripo_enabled') return 'false'
  if (key === 'tripo_base_url') return 'https://openapi.tripo3d.ai/v3'
  if (key === 'tripo_model') return 'P1-20260311'
  if (key === 'tripo_face_limit') return '5000'
  if (key === 'tripo_poll_interval_seconds') return '2'
  if (key === 'tripo_task_timeout_seconds') return '900'
  if (key === 'tripo_max_download_mb') return '64'
  return ''
}

export function defaultUiFieldValue(key: string): string {
  // Empty → client applies bundled `/wallpapers/default.webp` fallback.
  if (key === 'wallpaper_url') {
    return ''
  }
  if (key === 'wallpaper_blur') return '3'
  if (key === 'evocative_parallax') return 'true'
  if (key === 'evocative_dynamic_blur') return 'false'
  if (key === 'evocative_ripple') return 'false'
  if (key === 'evocative_fps') return '30'
  if (key === 'evocative_ripple_quality') return '0.85'
  if (key === 'music_enabled') return 'false'
  if (key === 'analytics_enabled') return 'true'
  if (key === 'pwa_enabled') return 'true'
  if (key === 'site_noindex') return 'false'
  // Align with backend normalize_visibility_policy("", false) → ai_full
  if (key === 'site_visibility_policy') return 'ai_full'
  // Clearable SEO / third-party analytics: empty = disabled (not env-injected)
  if (key === 'site_keywords') return ''
  if (key === 'site_og_image') return ''
  if (key === 'site_ai_intro') return ''
  if (key === 'ga_measurement_id') return ''
  if (key === 'umami_website_id') return ''
  if (key === 'umami_script_url') return ''
  if (key === 'site_footer_custom') return ''
  if (key === 'music_source') return 'netease'
  if (key === 'music_playlist_id') return ''
  if (key === 'proxy_enabled') return 'false'
  if (key === 'memory_saver_enabled') return 'false'
  if (key === 'agent_life_enabled') return 'false'
  if (key === 'proxy_url') return ''
  if (key === 'proxy_bypass') return ''
  if (key === 'gemini_base_url') return ''
  if (key === 'github_api_base_url') return ''
  return ''
}

export function mapConfigFields(
  fields: ConfigField[],
  getDefault: (key: string) => string,
  onlyKeys?: Set<string>,
): ConfigField[] {
  return fields.map((field) => {
    if (onlyKeys && !onlyKeys.has(field.key)) return field
    return { ...field, value: getDefault(field.key) }
  })
}
