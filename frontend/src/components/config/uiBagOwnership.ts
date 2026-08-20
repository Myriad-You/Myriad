/**
 * ui_config bag 字段归属（单一真相源）。
 * Section 组件 re-export 同名常量，避免与壳层 key 集漂移。
 */

/** UI（基础）页：不含 base_url（域名走 SiteUrlField 独立 API） */
export const UI_RESET_KEYS: readonly string[] = Object.freeze([
  'wallpaper_url',
  'wallpaper_blur',
  'site_title',
  'site_description',
  'site_favicon',
  'site_keywords',
  'site_og_image',
  'site_noindex',
  'site_visibility_policy',
  'site_ai_intro',
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

/**
 * 站点元数据 / SEO / 第三方统计 bag：保存后清缓存并 `refreshSiteMetadata`（无整页刷新）
 */
export const METADATA_SOFT_RELOAD_UI_BAG_KEYS: readonly string[] = Object.freeze([
  'site_title',
  'site_description',
  'site_favicon',
  'site_keywords',
  'site_og_image',
  'site_noindex',
  'site_visibility_policy',
  'site_ai_intro',
  'ga_measurement_id',
  'umami_website_id',
  'umami_script_url',
])

/**
 * 页脚 bag：保存后清 `/api/config/ui` 缓存并通知 SiteFooter 重载
 */
export const FOOTER_SOFT_RELOAD_UI_BAG_KEYS: readonly string[] = Object.freeze([
  'site_icp',
  'site_gongan',
  'cloud_sponsors',
  'site_footer_custom',
])

/** 数据及统计页：访客采集开关 + 第三方统计 bag */
export const PLATFORMS_UI_RESET_KEYS: readonly string[] = Object.freeze([
  'analytics_enabled',
  'ga_measurement_id',
  'umami_website_id',
  'umami_script_url',
])

/** 模块页：音乐播放器（库/报告/一言走独立 draft） */
export const MODULE_UI_RESET_KEYS: readonly string[] = Object.freeze([
  'music_enabled',
  'music_source',
  'music_playlist_id',
])

/** 高级页：内存节约 + 网络代理 + API 镜像 */
export const ADVANCED_RESET_KEYS: readonly string[] = Object.freeze([
  'memory_saver_enabled',
  'proxy_enabled',
  'proxy_url',
  'proxy_bypass',
  'gemini_base_url',
  'github_api_base_url',
])

/**
 * AI 页：Agent 生命总开关。字段仍存在 ui bag 里，但控件挂在 Lite / Pro 旁边——
 * 开口走 Lite、设定引导走 Pro，前提和开关不该分在两页。
 */
export const AI_UI_RESET_KEYS: readonly string[] = Object.freeze([
  'agent_life_enabled',
])

/** 全量重置时允许写入的 bag key（不含 base_url） */
export const ALL_OWNED_UI_BAG_KEYS: readonly string[] = Object.freeze([
  ...UI_RESET_KEYS,
  ...PLATFORMS_UI_RESET_KEYS,
  ...MODULE_UI_RESET_KEYS,
  ...AI_UI_RESET_KEYS,
  ...ADVANCED_RESET_KEYS,
])

/**
 * 变更后需要后端热重载（`reloadSystemConfig` / POST reload-config）的 bag key。
 * 代理与 API 镜像影响出站客户端；生命开关影响后端缓存的那份配置。
 * 不再触发整页 `location.reload`。
 */
export const RUNTIME_RELOAD_UI_BAG_KEYS: readonly string[] = Object.freeze([
  ...ADVANCED_RESET_KEYS,
  ...AI_UI_RESET_KEYS,
])

/**
 * 壁纸 / Evocative 动效：不必整页硬刷，但要清 UI 配置缓存并 `loadWallpaper`
 *（AppLayout 默认只在 mount 读一次）。
 */
export const WALLPAPER_SOFT_RELOAD_UI_BAG_KEYS: readonly string[] = Object.freeze([
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

interface ConfigShape {
  platforms: unknown
  auto_fetch: unknown
  ai_config: unknown
  ui_config?: { config_fields?: Array<{ key: string; value: string }> }
}

/**
 * 是否需要整页 `window.location.reload`。
 *
 * 主配置表单里 AI / 平台 / 自动刷新 / 代理镜像均已改为软路径：
 * - AI / auto_fetch / platforms → 只落库 + toast（platforms 另清 library 缓存）
 * - proxy / API 镜像 → `configChangesNeedRuntimeReload`（后端热重载，无整页刷新）
 * - 壁纸 / Evocative → `configChangesNeedWallpaperReload`
 *
 * 保留此函数与 hard-reload 分支，供未来进程级变更使用；当前主路径恒为 false。
 */
export function configChangesNeedHardReload(
  _next: ConfigShape,
  _prev: ConfigShape,
  _deepEqual: (a: unknown, b: unknown) => boolean,
): boolean {
  return false
}

/**
 * 代理 / API 镜像 bag 变更 → 调用 `reloadSystemConfig`，**不**整页刷新。
 */
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

/**
 * 平台配置变更 → 软保存后应失效 library 相关请求缓存，避免读到保存前数据。
 */
export function configChangesNeedPlatformsCacheInvalidation(
  next: { platforms: unknown },
  prev: { platforms: unknown },
  deepEqual: (a: unknown, b: unknown) => boolean,
): boolean {
  return !deepEqual(next.platforms, prev.platforms)
}

/** 壁纸 URL / 模糊 / Evocative 开关变更 → 软刷新壁纸层（非 hard reload） */
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

/** 页脚备案 / 云商标 / 自定义项变更 → SiteFooter 软重载 */
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

/** 站点标题 / 描述 / 图标 / SEO 变更 → 软刷新 document meta（非 hard reload） */
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

/** PWA 开关：保存后注册/注销 Service Worker（非 hard reload） */
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
