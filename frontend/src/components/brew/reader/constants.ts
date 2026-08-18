/**
 * Brew 阅读器共享常量
 */

import type { FontOption, LayoutOption, ThemeConfig, ThemeKey } from './types'

// 字体选项
export const FONT_OPTIONS: FontOption[] = [
  {
    id: 'serif',
    labelKey: 'fontSerif',
    family: '"Noto Serif SC", "Source Han Serif SC", "Songti SC", serif',
  },
  {
    id: 'sans',
    labelKey: 'fontSans',
    family: '"Noto Sans SC", "Source Han Sans SC", "PingFang SC", sans-serif',
  },
  {
    id: 'system',
    labelKey: 'fontSystem',
    family: '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
  },
]

// 主题配置
export const THEMES: Record<ThemeKey, ThemeConfig> = {
  light: {
    bg: 'bg-[#f8f5ec]',
    text: 'text-[#2c2c2c]',
    secondary: 'text-[#666]',
    border: 'border-[#e8e2d4]',
    surface: 'glass-surface glass-80',
    surfaceSolid: 'glass-surface glass-solid',
    accent: '#b8860b',
    icon: 'L',
  },
  sepia: {
    bg: 'bg-[#f4ecd8]',
    text: 'text-[#3d3229]',
    secondary: 'text-[#6b5d4d]',
    border: 'border-[#d4c8b0]',
    surface: 'glass-surface glass-80',
    surfaceSolid: 'glass-surface glass-solid',
    accent: '#8b6914',
    icon: 'S',
  },
  dark: {
    bg: 'bg-[#1a1a1a]',
    text: 'text-[#c9c9c9]',
    secondary: 'text-[#888]',
    border: 'border-[#333]',
    surface: 'glass-surface glass-80',
    surfaceSolid: 'glass-surface glass-solid',
    accent: '#fbbf24',
    icon: 'D',
  },
  night: {
    bg: 'bg-[#0d1117]',
    text: 'text-[#b8bfc7]',
    secondary: 'text-[#6e7681]',
    border: 'border-[#21262d]',
    surface: 'glass-surface glass-80',
    surfaceSolid: 'glass-surface glass-solid',
    accent: '#58a6ff',
    icon: 'N',
  },
}

export const THEME_ORDER: ThemeKey[] = ['light', 'sepia', 'dark', 'night']

// 布局宽度选项
export const LAYOUT_OPTIONS: LayoutOption[] = [
  { id: 'narrow', labelKey: 'layoutNarrow', width: 'max-w-3xl' },
  { id: 'wide', labelKey: 'layoutWide', width: 'max-w-4xl' },
]

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
// WebKit 优化：添加 will-change 提示 GPU 加速
export const STYLE_READER_CONTAINER = {
  transformOrigin: 'center bottom',
  willChange: 'opacity, transform',
} as const
export const STYLE_SCROLL_SMOOTH = {
  scrollBehavior: 'smooth' as const,
} as const
export const STYLE_MAX_HEIGHT_320 = { maxHeight: 'min(320px, 60vh)' } as const
export const STYLE_MAX_HEIGHT_60VH = { maxHeight: '60vh' } as const
