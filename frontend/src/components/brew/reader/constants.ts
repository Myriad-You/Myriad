/**
 * Brew 阅读器共享常量
 */

import type { FontOption, LayoutOption, ThemeConfig, ThemeKey } from './types'

// 字体选项
export const FONT_OPTIONS: FontOption[] = [
  { id: 'serif', labelKey: 'fontSerif', family: '"Noto Serif SC", "Source Han Serif SC", "Songti SC", serif' },
  { id: 'sans', labelKey: 'fontSans', family: '"Noto Sans SC", "Source Han Sans SC", "PingFang SC", sans-serif' },
  { id: 'system', labelKey: 'fontSystem', family: '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif' },
]

// 主题配置
export const THEMES: Record<ThemeKey, ThemeConfig> = {
  light: {
    bg: 'bg-[#f8f5ec]',
    text: 'text-[#2c2c2c]',
    secondary: 'text-[#666]',
    border: 'border-[#e8e2d4]',
    surface: 'bg-[#fff9f0]/80',
    surfaceSolid: 'bg-[#fff9f0]',
    accent: '#b8860b',
    icon: '☀️',
  },
  sepia: {
    bg: 'bg-[#f4ecd8]',
    text: 'text-[#3d3229]',
    secondary: 'text-[#6b5d4d]',
    border: 'border-[#d4c8b0]',
    surface: 'bg-[#faf4e6]/80',
    surfaceSolid: 'bg-[#faf4e6]',
    accent: '#8b6914',
    icon: '📜',
  },
  dark: {
    bg: 'bg-[#1a1a1a]',
    text: 'text-[#c9c9c9]',
    secondary: 'text-[#888]',
    border: 'border-[#333]',
    surface: 'bg-[#242424]/80',
    surfaceSolid: 'bg-[#242424]',
    accent: '#fbbf24',
    icon: '🌙',
  },
  night: {
    bg: 'bg-[#0d1117]',
    text: 'text-[#b8bfc7]',
    secondary: 'text-[#6e7681]',
    border: 'border-[#21262d]',
    surface: 'bg-[#161b22]/80',
    surfaceSolid: 'bg-[#161b22]',
    accent: '#58a6ff',
    icon: '🌑',
  },
}

export const THEME_ORDER: ThemeKey[] = ['light', 'sepia', 'dark', 'night']

// 布局宽度选项
export const LAYOUT_OPTIONS: LayoutOption[] = [
  { id: 'narrow', labelKey: 'layoutNarrow', width: 'max-w-3xl' },
  { id: 'wide', labelKey: 'layoutWide', width: 'max-w-4xl' },
]

// Framer Motion transition 配置常量
export const TRANSITION_FAST = { duration: 0.1 } as const
export const TRANSITION_NORMAL = { duration: 0.15 } as const
export const TRANSITION_SLOW = { duration: 0.2, ease: 'easeOut' as const } as const
export const TRANSITION_PANEL = { duration: 0.25, ease: 'easeOut' as const } as const

// 日期格式化选项常量
export const DATE_FORMAT_SHORT: Intl.DateTimeFormatOptions = {
  month: 'short',
  day: 'numeric',
}

export const DATE_FORMAT_FULL: Intl.DateTimeFormatOptions = {
  month: 'short',
  day: 'numeric',
  hour: '2-digit',
  minute: '2-digit',
}

// 静态 style 常量
export const STYLE_TRANSFORM_ORIGIN = { transformOrigin: 'center bottom' } as const
// WebKit 优化：添加 will-change 提示 GPU 加速
export const STYLE_READER_CONTAINER = {
  transformOrigin: 'center bottom',
  willChange: 'opacity, transform',
} as const
export const STYLE_SCROLL_SMOOTH = { scrollBehavior: 'smooth' as const } as const
export const STYLE_MAX_HEIGHT_320 = { maxHeight: 'min(320px, 60vh)' } as const
export const STYLE_MAX_HEIGHT_60VH = { maxHeight: '60vh' } as const
