import type { FontOption, LayoutOption, ThemeConfig, ThemeKey } from './types'

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

export const THEMES: Record<ThemeKey, ThemeConfig> = {
  light: {
    bg: 'bg-[#f8f5ec]',
    text: 'text-[#2c2c2c]',
    secondary: 'text-[#666]',
    border: 'brew-reader__line',
    surface: 'brew-reader__chip',
    surfaceSolid: 'brew-reader__chip is-solid',
    accent: '#b8860b',
    icon: 'L',
  },
  sepia: {
    bg: 'bg-[#f4ecd8]',
    text: 'text-[#3d3229]',
    secondary: 'text-[#6b5d4d]',
    border: 'brew-reader__line',
    surface: 'brew-reader__chip',
    surfaceSolid: 'brew-reader__chip is-solid',
    accent: '#8b6914',
    icon: 'S',
  },
  dark: {
    bg: 'bg-[#1a1a1a]',
    text: 'text-[#c9c9c9]',
    secondary: 'text-[#888]',
    border: 'brew-reader__line',
    surface: 'brew-reader__chip',
    surfaceSolid: 'brew-reader__chip is-solid',
    accent: '#fbbf24',
    icon: 'D',
  },
  night: {
    bg: 'bg-[#0d1117]',
    text: 'text-[#b8bfc7]',
    secondary: 'text-[#6e7681]',
    border: 'brew-reader__line',
    surface: 'brew-reader__chip',
    surfaceSolid: 'brew-reader__chip is-solid',
    accent: '#58a6ff',
    icon: 'N',
  },
}

export const THEME_ORDER: ThemeKey[] = ['light', 'sepia', 'dark', 'night']

export const LAYOUT_OPTIONS: LayoutOption[] = [
  { id: 'narrow', labelKey: 'layoutNarrow', width: 'max-w-3xl' },
  { id: 'wide', labelKey: 'layoutWide', width: 'max-w-4xl' },
]

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

// WebKit：will-change 提示 GPU。
export const STYLE_READER_CONTAINER = {
  transformOrigin: 'center bottom',
  willChange: 'opacity, transform',
} as const
export const STYLE_SCROLL_SMOOTH = {
  scrollBehavior: 'smooth' as const,
} as const
export const STYLE_MAX_HEIGHT_320 = { maxHeight: 'min(320px, 60vh)' } as const
export const STYLE_MAX_HEIGHT_60VH = { maxHeight: '60vh' } as const

export const READER_COMMENTS_PANEL_ID = 'brew-comments-panel'
export const READER_COMMENTS_TITLE_ID = 'brew-comments-title'
export const READER_TOOL_SHEET_ID = 'brew-reader-tool-sheet'
export const READER_TOOL_TITLE_ID = 'brew-reader-tool-title'
export const READER_TOC_PANEL_ID = 'brew-reader-toc-panel'
export const READER_TOC_TITLE_ID = 'brew-reader-toc-title'
export const READER_ANNOTATIONS_PANEL_ID = 'brew-reader-annotations-panel'
export const READER_ANNOTATIONS_TITLE_ID = 'brew-reader-annotations-title'
export const READER_PODCAST_PANEL_ID = 'brew-reader-podcast-panel'
export const READER_PODCAST_TITLE_ID = 'brew-reader-podcast-title'
export const READER_MOBILE_CONTROLS_ID = 'brew-reader-mobile-controls'
