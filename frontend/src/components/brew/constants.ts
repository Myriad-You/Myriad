/**
 * Brew 模块共享常量
 */

import type { CardSize } from '../../types/brew'

// ==================== 预设分类 ====================

/** 预置分类的数据库存储值（与后端保持一致） */
export const PRESET_CATEGORY_DB_VALUES = ['友情链接', '我']

// ==================== 默认值 ====================

/** 默认主题色（用于无图标或提取失败的情况） */
export const DEFAULT_THEME_COLOR = '#6b7280'

/** 卡片尺寸顺序 */
export const SIZE_ORDER: CardSize[] = ['tiny', 'mini', 'full']

/** 尺寸变化阈值（像素） */
export const RESIZE_THRESHOLD = 100

/** 尺寸对应的 row-span */
export const SIZE_TO_ROWS: Record<CardSize, number> = {
  full: 8, // 8 × 24px = 192px
  mini: 4, // 4 × 24px = 96px
  tiny: 2, // 2 × 24px = 48px
}

/** 短文阈值（字符数）- 低于此值视为简讯/短文 */
export const SHORT_CONTENT_THRESHOLD = 280

// ==================== 动画配置 ====================

/** Spring 动画 - 快速 */
export const SPRING_SNAPPY = { type: 'spring', stiffness: 400, damping: 25 } as const

/** Spring 动画 - 平滑 */
export const SPRING_SMOOTH = { type: 'spring', stiffness: 350, damping: 28 } as const

/** 过渡动画 - 快速 */
export const TRANSITION_QUICK = { duration: 0.12 } as const

/** 过渡动画 - 正常 */
export const TRANSITION_NORMAL = { duration: 0.15 } as const

/** 过渡动画 - 慢速 */
export const TRANSITION_SLOW = { duration: 0.25, ease: 'easeOut' } as const

// ==================== API 配置 ====================

/** API 基础 URL */
export const API_URL = import.meta.env.PUBLIC_API_URL || ''

// ==================== 工具函数 ====================

/**
 * 处理图标 URL - 如果是外部 URL 则通过代理访问
 */
export function getIconUrl(iconUrl: string | null): string | null {
  if (!iconUrl)
    return null
  if (iconUrl.startsWith('/api/')) {
    return `${API_URL}${iconUrl}`
  }
  if (iconUrl.startsWith('http://') || iconUrl.startsWith('https://')) {
    return `${API_URL}/api/proxy/image?url=${encodeURIComponent(iconUrl)}`
  }
  return iconUrl
}

/**
 * 处理图片 URL - 封面图等外部图片通过代理访问
 */
export function getImageUrl(imageUrl: string | null): string | null {
  if (!imageUrl)
    return null
  if (imageUrl.startsWith('/api/') || imageUrl.startsWith(`${API_URL}/api/`)) {
    return imageUrl.startsWith('/api/') ? `${API_URL}${imageUrl}` : imageUrl
  }
  if (imageUrl.startsWith('http://') || imageUrl.startsWith('https://')) {
    return `${API_URL}/api/proxy/image?url=${encodeURIComponent(imageUrl)}`
  }
  return imageUrl
}

/**
 * 清理 HTML 标签
 */
export function stripHtml(html: string | null): string {
  if (!html)
    return ''
  return html.replace(/<[^>]*>/g, '').replace(/&nbsp;/g, ' ').trim()
}

/**
 * 提取摘要纯文本
 */
export function getPlainText(html: string | null): string {
  if (!html)
    return ''
  return html.replace(/<[^>]*>/g, '').slice(0, 200)
}

/**
 * 提取完整纯文本 - 用于短文判断和显示
 */
export function getFullPlainText(html: string | null): string {
  if (!html)
    return ''
  return html.replace(/<[^>]*>/g, '').trim()
}

/**
 * 判断是否为 base64 图片数据
 */
export function isBase64Image(str: string | null): boolean {
  if (!str)
    return false
  return str.startsWith('data:image/')
}

/**
 * 从 base64 提取 MIME 类型和扩展名
 */
export function getBase64Info(base64: string): { mime: string, ext: string } {
  const match = base64.match(/^data:(image\/\w+);base64,/)
  if (match) {
    const mime = match[1]
    const ext = mime.split('/')[1] || 'png'
    return { mime, ext }
  }
  return { mime: 'image/png', ext: 'png' }
}

/**
 * 获取源的主题色 - 优先使用数据库中存储的 theme_color
 */
export function getSourceColor(source: { theme_color?: string | null }): string {
  if (source.theme_color) {
    return source.theme_color
  }
  return DEFAULT_THEME_COLOR
}
