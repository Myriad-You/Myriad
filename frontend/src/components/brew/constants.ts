import { API_URL as CONFIG_API_URL } from '../../config'
import { proxyImageUrl } from '../../utils/proxyImageUrl'

/** Canonical stored values. Display copy lives in locale JSON. */
export const BREW_FRIEND_LINK_CATEGORY = '友情链接'
export const BREW_MINE_CATEGORY = '我'

const FRIEND_LINK_ALIASES = new Set([
  BREW_FRIEND_LINK_CATEGORY,
  '友情連結',
  'friend_links',
  'friend-links',
  'friend links',
])

const MINE_ALIASES = new Set([
  BREW_MINE_CATEGORY,
  'mine',
  'own',
  'me',
  'my',
])

export const PRESET_CATEGORY_DB_VALUES: string[] = [
  ...FRIEND_LINK_ALIASES,
  ...MINE_ALIASES,
]

function normalizeCategoryToken(value: string): string {
  return value.trim().toLowerCase()
}

export function isFriendLinkCategory(value: string | null | undefined): boolean {
  const raw = value?.trim() ?? ''
  if (!raw) return false
  return (
    FRIEND_LINK_ALIASES.has(raw) ||
    FRIEND_LINK_ALIASES.has(normalizeCategoryToken(raw))
  )
}

export function isMineCategory(value: string | null | undefined): boolean {
  const raw = value?.trim() ?? ''
  if (!raw) return false
  return MINE_ALIASES.has(raw) || MINE_ALIASES.has(normalizeCategoryToken(raw))
}

export function brewCategoryParts(
  category: string | null | undefined,
): string[] {
  if (!category) return []
  return category
    .split(',')
    .map((p) => p.trim())
    .filter(Boolean)
}

/** category 含「我」且非 admin_only 才算自有，才可做文章级 SEO。 */
export function isOwnBrewSource(source: {
  category?: string | null
  admin_only?: boolean
} | null | undefined): boolean {
  if (!source) return false
  // admin_only 源不公开收录
  if (source.admin_only) return false
  return brewCategoryParts(source.category).some(isMineCategory)
}

/** 忽略预置分类后的第一个真实分类；排序与分类页标题共用。 */
export function brewMainCategory(
  category: string | null | undefined,
  fallback: string,
): string {
  const parts = brewCategoryParts(category)
  const main = parts.find(
    (c) => !isFriendLinkCategory(c) && !isMineCategory(c),
  )
  return main || fallback
}

export function brewOwnItemPath(itemId: number | string): string {
  return `/brew/item/${encodeURIComponent(String(itemId))}`
}

export const DEFAULT_THEME_COLOR = '#6b7280'

export const API_URL = CONFIG_API_URL

/** 仅 must-proxy 走 `/api/proxy/image`。 */
export function getIconUrl(iconUrl: string | null): string | null {
  if (!iconUrl) return null
  if (iconUrl.startsWith(`${API_URL}/api/`)) {
    return iconUrl
  }
  if (iconUrl.startsWith('/api/')) {
    return `${API_URL}${iconUrl}`
  }
  return proxyImageUrl(iconUrl) ?? iconUrl
}

/** 规则同 getIconUrl。 */
export function getImageUrl(imageUrl: string | null): string | null {
  if (!imageUrl) return null
  if (imageUrl.startsWith(`${API_URL}/api/`)) {
    return imageUrl
  }
  if (imageUrl.startsWith('/api/')) {
    return `${API_URL}${imageUrl}`
  }
  return proxyImageUrl(imageUrl) ?? imageUrl
}

export function getPlainText(html: string | null): string {
  if (!html) return ''
  return html.replaceAll(/<[^>]*>/g, '').slice(0, 200)
}

/** 只产出 #rrggbb，`${color}30` 才是合法 CSS。 */
export function normalizeThemeColor(
  color: string | null | undefined,
  fallback = DEFAULT_THEME_COLOR,
): string {
  if (!color || typeof color !== 'string') return fallback
  const t = color.trim()
  if (/^#([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/i.test(t)) {
    if (t.length === 4) {
      // #rgb → #rrggbb
      return `#${t[1]}${t[1]}${t[2]}${t[2]}${t[3]}${t[3]}`.toLowerCase()
    }
    return t.slice(0, 7).toLowerCase() // drop #rrggbbaa alpha
  }
  return fallback
}
