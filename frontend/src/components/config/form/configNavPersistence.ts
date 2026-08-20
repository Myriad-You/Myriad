/**
 * 配置页导航位置持久化（sessionStorage）。
 * 用于：切换分类后 F5 / 保存触发硬刷新时恢复所在 section 与滚动位置。
 */

import { LEGACY_CONFIG_SECTION_MAP } from './defaults'

export const CONFIG_NAV_STORAGE_KEY = 'myriad_config_nav_v1'

/** 已知一级分类（与 useConfigNavigation.quickAccessItems 对齐） */
export const CONFIG_NAV_SECTIONS = [
  'platforms',
  'ai',
  'tripo',
  'basic',
  'oauth',
  'federation',
  'permissions',
  'users',
  'notifications',
  'modules',
  'advanced',
  'about',
] as const

export type ConfigNavSection = (typeof CONFIG_NAV_SECTIONS)[number]

export interface ConfigNavPersisted {
  section: string
  mobilePane?: 'nav' | 'section'
  platformFocus?: string | null
  /** window 纵向滚动 */
  scrollY?: number
}

function isKnownSection(section: string, isAdmin: boolean): boolean {
  if (section === 'federation' && !isAdmin) return false
  return (CONFIG_NAV_SECTIONS as readonly string[]).includes(section)
}

export function normalizeConfigSection(
  raw: string | null | undefined,
  isAdmin: boolean,
): string | null {
  if (!raw) return null
  const next = LEGACY_CONFIG_SECTION_MAP[raw] ?? raw
  return isKnownSection(next, isAdmin) ? next : null
}

export function loadConfigNavPersisted(): ConfigNavPersisted | null {
  if (typeof window === 'undefined') return null
  try {
    const raw = sessionStorage.getItem(CONFIG_NAV_STORAGE_KEY)
    if (!raw) return null
    const parsed = JSON.parse(raw) as Partial<ConfigNavPersisted>
    if (!parsed || typeof parsed.section !== 'string') return null
    return {
      section: parsed.section,
      mobilePane:
        parsed.mobilePane === 'nav' || parsed.mobilePane === 'section'
          ? parsed.mobilePane
          : undefined,
      platformFocus:
        parsed.platformFocus === null ||
        typeof parsed.platformFocus === 'string'
          ? parsed.platformFocus
          : undefined,
      scrollY:
        typeof parsed.scrollY === 'number' &&
        Number.isFinite(parsed.scrollY) &&
        parsed.scrollY >= 0
          ? parsed.scrollY
          : undefined,
    }
  } catch {
    return null
  }
}

export function saveConfigNavPersisted(
  patch: Partial<ConfigNavPersisted>,
): void {
  if (typeof window === 'undefined') return
  try {
    const prev = loadConfigNavPersisted() ?? { section: 'platforms' }
    const next: ConfigNavPersisted = {
      section: patch.section ?? prev.section,
      mobilePane:
        patch.mobilePane !== undefined ? patch.mobilePane : prev.mobilePane,
      platformFocus:
        patch.platformFocus !== undefined
          ? patch.platformFocus
          : prev.platformFocus,
      scrollY: patch.scrollY !== undefined ? patch.scrollY : prev.scrollY,
    }
    sessionStorage.setItem(CONFIG_NAV_STORAGE_KEY, JSON.stringify(next))
  } catch {
    /* private mode / quota */
  }
}

/** 硬刷新前调用：把当前 window 滚动写进 session */
export function snapshotConfigNavScroll(): void {
  if (typeof window === 'undefined') return
  saveConfigNavPersisted({ scrollY: window.scrollY || window.pageYOffset || 0 })
}

/**
 * 解析初始 section：URL ?section= 优先，否则 sessionStorage，默认 platforms。
 */
export function resolveInitialConfigSection(isAdmin: boolean): string {
  if (typeof window === 'undefined') return 'platforms'
  try {
    const params = new URLSearchParams(window.location.search)
    const fromUrl = normalizeConfigSection(params.get('section'), isAdmin)
    if (fromUrl) return fromUrl
    const stored = loadConfigNavPersisted()
    const fromStore = normalizeConfigSection(stored?.section, isAdmin)
    if (fromStore) return fromStore
  } catch {
    /* ignore */
  }
  return 'platforms'
}

/** 用 replaceState 同步 ?section=，保留其余 query */
export function syncConfigSectionToUrl(section: string): void {
  if (typeof window === 'undefined') return
  try {
    const url = new URL(window.location.href)
    if (url.searchParams.get('section') === section) return
    url.searchParams.set('section', section)
    if (section !== 'ai') url.searchParams.delete('page')
    window.history.replaceState(window.history.state, '', url.toString())
  } catch {
    /* ignore */
  }
}
