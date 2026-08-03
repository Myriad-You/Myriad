/**
 * Control Island 共享常量
 * 为 Brew 等模块的控制岛提供统一的样式基础
 */

import { API_URL as CONFIG_API_URL } from '../../../config'
import { proxyImageUrl } from '../../../utils/proxyImageUrl'

// 玻璃态容器样式

/** 标准玻璃容器 — 玻璃底走 --surface 令牌（跟随主题）+ 精细边框 + 多层投影 */
export const ISLAND_GLASS = [
  'rounded-2xl',
  'glass-surface', // 底色 80% + 滤镜走令牌，替代 bg-white/80 dark:bg-neutral-900/80 backdrop-blur-2xl
  'border border-white/60 dark:border-white/8',
  'shadow-[inset_0_0.5px_0_0_rgba(255,255,255,0.15),0_8px_32px_-8px_rgba(0,0,0,0.12),0_2px_8px_-2px_rgba(0,0,0,0.06)]',
  'dark:shadow-[inset_0_0.5px_0_0_rgba(255,255,255,0.08),0_8px_32px_-8px_rgba(0,0,0,0.4),0_2px_8px_-2px_rgba(0,0,0,0.2)]',
].join(' ')

/** 编辑模式玻璃容器 — 略带色调 */
export const ISLAND_GLASS_EDIT = [
  'rounded-2xl',
  'glass-surface',
  'border border-gray-200/60 dark:border-white/8',
  'shadow-[inset_0_0.5px_0_0_rgba(255,255,255,0.1),0_8px_32px_-8px_rgba(0,0,0,0.12),0_2px_8px_-2px_rgba(0,0,0,0.06)]',
  'dark:shadow-[inset_0_0.5px_0_0_rgba(255,255,255,0.05),0_8px_32px_-8px_rgba(0,0,0,0.4),0_2px_8px_-2px_rgba(0,0,0,0.2)]',
].join(' ')

// 动画 Transition

export const SPRING_SNAPPY = {
  type: 'spring',
  stiffness: 400,
  damping: 25,
} as const
export const SPRING_SMOOTH = {
  type: 'spring',
  stiffness: 350,
  damping: 28,
} as const
export const TRANSITION_QUICK = { duration: 0.12 } as const
export const TRANSITION_NORMAL = { duration: 0.15 } as const
export const TRANSITION_SLOW = { duration: 0.25, ease: 'easeOut' } as const

// 按钮样式

/** 标准操作按钮 */
export const ISLAND_BTN = [
  'flex items-center justify-center gap-1.5',
  'h-9 px-3 rounded-xl',
  'text-gray-500 dark:text-gray-400',
  'hover:text-gray-700 dark:hover:text-gray-200',
  'hover:bg-black/4 dark:hover:bg-white/6',
  'hover:scale-102',
  'active:scale-95',
  'transition-all duration-200 ease-out',
].join(' ')

/** 强调操作按钮（主色调 CTA） */
export const ISLAND_BTN_PRIMARY = [
  'flex items-center justify-center gap-1.5',
  'h-9 px-4 rounded-xl',
  'bg-linear-to-r from-indigo-500 to-indigo-600',
  'hover:from-indigo-600 hover:to-indigo-700',
  'text-white text-sm font-medium',
  'shadow-sm shadow-indigo-500/25',
  'hover:shadow-md hover:shadow-indigo-500/30',
  'hover:scale-102',
  'active:scale-97',
  'transition-all duration-200 ease-out',
  'disabled:opacity-40 disabled:cursor-not-allowed disabled:shadow-none',
].join(' ')

/** 危险操作按钮 */
export const ISLAND_BTN_DANGER = [
  'flex items-center justify-center gap-1.5',
  'h-9 px-3 rounded-xl',
  'text-red-500 dark:text-red-400',
  'hover:bg-red-500/10 dark:hover:bg-red-500/15',
  'hover:scale-102',
  'active:scale-95',
  'transition-all duration-200 ease-out',
  'disabled:opacity-30 disabled:cursor-not-allowed',
].join(' ')

// 输入框样式

/** 透明输入框（内嵌在控制岛中） */
export const ISLAND_INPUT = [
  'bg-transparent border-none outline-none ring-0',
  'text-sm text-gray-700 dark:text-gray-200',
  'placeholder:text-gray-400 dark:placeholder:text-gray-500',
  'focus:outline-none focus:ring-0 focus:border-none',
  'appearance-none',
].join(' ')

/** 输入框容器内联样式（消除浏览器默认阴影） */
export const ISLAND_INPUT_STYLE: React.CSSProperties = {
  boxShadow: 'none',
  background: 'transparent',
  WebkitAppearance: 'none',
}

// 分隔线

export const ISLAND_DIVIDER =
  'w-px h-6 bg-linear-to-b from-transparent via-gray-300/50 to-transparent dark:via-white/10'

// 徽章

export const ISLAND_BADGE =
  'text-[10px] font-medium tabular-nums text-gray-400 dark:text-gray-500 px-1.5'

// 下拉选择框

export const ISLAND_SELECT = [
  'h-9 px-3 rounded-xl',
  'bg-transparent text-sm',
  'text-gray-600 dark:text-gray-300',
  'border-none outline-none cursor-pointer',
  'hover:bg-black/4 dark:hover:bg-white/6',
  'transition-colors duration-150',
].join(' ')

// API URL

export const API_URL = CONFIG_API_URL

/**
 * Dual-path icon URL: hotlink CDNs via proxy; otherwise original https.
 * Already-proxied and same-origin API paths are not re-wrapped.
 */
export function getIconUrl(iconUrl: string | null | undefined): string | null {
  if (!iconUrl) return null
  if (iconUrl.startsWith(`${API_URL}/api/`)) return iconUrl
  if (iconUrl.includes('/api/proxy/image')) return iconUrl
  if (iconUrl.startsWith('/api/')) {
    return `${API_URL}${iconUrl}`
  }
  return proxyImageUrl(iconUrl) ?? iconUrl
}
