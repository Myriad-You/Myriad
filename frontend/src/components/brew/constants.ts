/**
 * Brew 模块共享常量
 */

import type { CardSize } from '../../types/brew'
import { API_URL as CONFIG_API_URL } from '../../config'
import { proxyImageUrl } from '../../utils/proxyImageUrl'

// 预设分类

/**
 * 预置分类的数据库存储值（与后端 SEO / 前端导航一致，不要改文案）。
 * - 友情链接：外部站点入口，非自有内容
 * - 我：站主自有内容（唯一可做文章级 SEO 的分类）
 */
export const BREW_FRIEND_LINK_CATEGORY = '友情链接'
export const BREW_MINE_CATEGORY = '我'

/** 预置分类的数据库存储值（与后端保持一致） */
export const PRESET_CATEGORY_DB_VALUES: string[] = [
  BREW_FRIEND_LINK_CATEGORY,
  BREW_MINE_CATEGORY,
]

/** 拆分 source.category（支持逗号分隔多分类） */
export function brewCategoryParts(
  category: string | null | undefined,
): string[] {
  if (!category) return []
  return category
    .split(',')
    .map((p) => p.trim())
    .filter(Boolean)
}

/**
 * 是否为站主自有内容源（category 含「我」）。
 * 友情链接、其它 RSS 订阅源一律不算自有，不得做文章级 SEO。
 */
export function isOwnBrewSource(source: {
  category?: string | null
  admin_only?: boolean
} | null | undefined): boolean {
  if (!source) return false
  // 仅管理员可见的源不公开收录
  if (source.admin_only) return false
  return brewCategoryParts(source.category).includes(BREW_MINE_CATEGORY)
}

/** 自有文章的站内规范路径（sitemap / OG / 分享） */
export function brewOwnItemPath(itemId: number | string): string {
  return `/brew/item/${encodeURIComponent(String(itemId))}`
}

// 默认值

/** 默认主题色（用于无图标或提取失败的情况） */
export const DEFAULT_THEME_COLOR = '#6b7280'

/** 尺寸对应的 row-span */
export const SIZE_TO_ROWS: Record<CardSize, number> = {
  full: 8, // 8 × 24px = 192px
  mini: 4, // 4 × 24px = 96px
  tiny: 2, // 2 × 24px = 48px
}

/** 短文阈值（字符数）- 低于此值视为简讯/短文 */
const SHORT_CONTENT_THRESHOLD = 280

// API 配置

/** API 基础 URL */
export const API_URL = CONFIG_API_URL

// 工具函数

/**
 * Dual-path: only must-proxy hosts go through `/api/proxy/image`.
 * RSS / personal-blog favicons and covers use the original https URL.
 */
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

/**
 * Dual-path cover/image helper (same rules as getIconUrl).
 */
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

/**
 * 清理 HTML 标签
 */
export function stripHtml(html: string | null): string {
  if (!html) return ''
  return html
    .replace(/<[^>]*>/g, '')
    .replace(/&nbsp;/g, ' ')
    .trim()
}

/**
 * 提取摘要纯文本
 */
export function getPlainText(html: string | null): string {
  if (!html) return ''
  return html.replace(/<[^>]*>/g, '').slice(0, 200)
}

/**
 * 提取完整纯文本 - 用于短文判断和显示
 */
function getFullPlainText(html: string | null): string {
  if (!html) return ''
  return html.replace(/<[^>]*>/g, '').trim()
}

/**
 * 判断 HTML 是否可能是「短文」（纯文本 < SHORT_CONTENT_THRESHOLD）
 * 不分配全文 strip 结果：HTML 足够长时纯文本不可能仍 < 阈值
 *
 * 启发式：标签占比再高，HTML 长度若 ≥ threshold * 8，剥标签后仍几乎必 ≥ threshold
 * （保守系数，保证不把真正的短文误判为长文）
 */
function isLikelyLongHtml(
  html: string | null,
  threshold: number = SHORT_CONTENT_THRESHOLD,
): boolean {
  if (!html) return false
  return html.length >= threshold * 8
}

/**
 * 为列表卡片计算短文展示文本。
 * 长 HTML 直接返回 null（一定不是短文），避免无意义的全文 strip 占内存。
 * 行为与「先 getFullPlainText 再比长度」一致。
 */
export function getShortContentText(
  content: string | null,
  summary: string | null,
  threshold: number = SHORT_CONTENT_THRESHOLD,
): string | null {
  const html = content || summary
  if (!html) return null
  if (isLikelyLongHtml(html, threshold)) return null
  const plain = getFullPlainText(html)
  if (!plain || plain.length >= threshold) return null
  return plain
}

/**
 * Normalize theme_color to #rrggbb so `${color}30` alpha suffixes stay valid CSS.
 * Non-hex values (named colors, rgb()) break card gradients/shadows.
 */
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
    return t.slice(0, 7).toLowerCase() // drop alpha nibble if #rrggbbaa
  }
  return fallback
}

/**
 * 获取源的主题色 - 优先使用数据库中存储的 theme_color
 */
export function getSourceColor(source: {
  theme_color?: string | null
}): string {
  return normalizeThemeColor(source.theme_color, DEFAULT_THEME_COLOR)
}
