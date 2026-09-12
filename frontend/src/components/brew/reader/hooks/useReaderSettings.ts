import type {
  FontOption,
  LayoutKey,
  LayoutOption,
  ThemeConfig,
  ThemeKey,
} from '../types'
import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  isExlight,
  useAnimationLevel,
} from '../../../../hooks/useAnimationLevel'
import {
  getIsDarkMode,
  subscribeToTheme,
} from '../../../../utils/themeSubscriber'
import { FONT_OPTIONS, LAYOUT_OPTIONS, THEME_ORDER, THEMES } from '../constants'

function getStoredSettings() {
  try {
    const stored = localStorage.getItem('brew-reader-settings')
    if (stored) return JSON.parse(stored)
  } catch {}
  return null
}

function saveSettings(settings: object) {
  try {
    localStorage.setItem('brew-reader-settings', JSON.stringify(settings))
  } catch {}
}

export interface UseReaderSettingsReturn {
  fontSize: number
  lineHeight: number
  fontFamily: string
  theme: ThemeKey
  layout: LayoutKey

  currentTheme: ThemeConfig
  currentFont: FontOption
  currentLayout: LayoutOption
  isDark: boolean

  setFontSize: (size: number) => void
  setLineHeight: (height: number) => void
  setFontFamily: (family: string) => void
  setTheme: (theme: ThemeKey) => void
  setLayout: (layout: LayoutKey) => void
  adjustFontSize: (delta: number) => void
  adjustLineHeight: (delta: number) => void
  cycleTheme: () => void
  cycleFont: () => void
  cycleLayout: () => void
}

export function useReaderSettings(): UseReaderSettingsReturn {
  const anim = useAnimationLevel()

  const [fontSize, setFontSize] = useState(
    () => getStoredSettings()?.fontSize ?? 18,
  )
  const [lineHeight, setLineHeight] = useState(
    () => getStoredSettings()?.lineHeight ?? 1.8,
  )
  const [fontFamily, setFontFamily] = useState(
    () => getStoredSettings()?.fontFamily ?? 'serif',
  )
  // 首次渲染直接读 DOM 主题，避免 light→dark 闪烁。
  const [theme, setTheme] = useState<ThemeKey>(() =>
    getIsDarkMode() ? 'dark' : 'light',
  )
  const [layout, setLayout] = useState<LayoutKey>(
    () => getStoredSettings()?.layout ?? 'narrow',
  )

  useEffect(() => {
    return subscribeToTheme((isDark) => {
      setTheme(isDark ? 'dark' : 'light')
    })
  }, [])

  // 主题不保存，每次跟随系统。
  useEffect(() => {
    saveSettings({ fontSize, lineHeight, fontFamily, layout })
  }, [fontSize, lineHeight, fontFamily, layout])

  const currentTheme = useMemo(() => {
    const base = THEMES[theme]
    if (isExlight(anim)) {
      return { ...base, surface: base.surfaceSolid }
    }
    return base
  }, [theme, anim])
  const currentFont = useMemo(
    () => FONT_OPTIONS.find((f) => f.id === fontFamily) ?? FONT_OPTIONS[0],
    [fontFamily],
  )
  const currentLayout = useMemo(
    () => LAYOUT_OPTIONS.find((l) => l.id === layout) ?? LAYOUT_OPTIONS[0],
    [layout],
  )
  const isDark = useMemo(() => theme === 'dark' || theme === 'night', [theme])

  const adjustFontSize = useCallback((delta: number) => {
    setFontSize((prev: number) => Math.max(14, Math.min(28, prev + delta)))
  }, [])

  const adjustLineHeight = useCallback((delta: number) => {
    setLineHeight((prev: number) =>
      Math.max(1.4, Math.min(2.4, +(prev + delta).toFixed(1))),
    )
  }, [])

  const cycleTheme = useCallback(() => {
    setTheme((prev) => {
      const currentIndex = THEME_ORDER.indexOf(prev)
      return THEME_ORDER[(currentIndex + 1) % THEME_ORDER.length]
    })
  }, [])

  const cycleFont = useCallback(() => {
    setFontFamily((prev: string) => {
      const currentIndex = FONT_OPTIONS.findIndex((f) => f.id === prev)
      return FONT_OPTIONS[(currentIndex + 1) % FONT_OPTIONS.length].id
    })
  }, [])

  const cycleLayout = useCallback(() => {
    setLayout((prev) => {
      const currentIndex = LAYOUT_OPTIONS.findIndex((l) => l.id === prev)
      return LAYOUT_OPTIONS[(currentIndex + 1) % LAYOUT_OPTIONS.length].id
    })
  }, [])

  return {
    fontSize,
    lineHeight,
    fontFamily,
    theme,
    layout,

    currentTheme,
    currentFont,
    currentLayout,
    isDark,

    setFontSize,
    setLineHeight,
    setFontFamily,
    setTheme,
    setLayout,
    adjustFontSize,
    adjustLineHeight,
    cycleTheme,
    cycleFont,
    cycleLayout,
  }
}
