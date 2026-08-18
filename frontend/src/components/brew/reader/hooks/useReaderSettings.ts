/**
 * 阅读器设置 Hook
 * 管理字体、字号、行高、主题、布局等阅读偏好设置
 */

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

// 从 localStorage 读取设置
function getStoredSettings() {
  try {
    const stored = localStorage.getItem('brew-reader-settings')
    if (stored) return JSON.parse(stored)
  } catch {}
  return null
}

// 保存设置到 localStorage
function saveSettings(settings: object) {
  try {
    localStorage.setItem('brew-reader-settings', JSON.stringify(settings))
  } catch {}
}

export interface UseReaderSettingsReturn {
  // 状态
  fontSize: number
  lineHeight: number
  fontFamily: string
  theme: ThemeKey
  layout: LayoutKey

  // 计算值
  currentTheme: ThemeConfig
  currentFont: FontOption
  currentLayout: LayoutOption
  isDark: boolean

  // 操作
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

  // 阅读设置状态 - 使用懒初始化，避免每次渲染都读 localStorage
  const [fontSize, setFontSize] = useState(
    () => getStoredSettings()?.fontSize ?? 18,
  )
  const [lineHeight, setLineHeight] = useState(
    () => getStoredSettings()?.lineHeight ?? 1.8,
  )
  const [fontFamily, setFontFamily] = useState(
    () => getStoredSettings()?.fontFamily ?? 'serif',
  )
  // 主题懒初始化：首次渲染直接读 DOM，避免 light→dark 的闪烁
  const [theme, setTheme] = useState<ThemeKey>(() =>
    getIsDarkMode() ? 'dark' : 'light',
  )
  const [layout, setLayout] = useState<LayoutKey>(
    () => getStoredSettings()?.layout ?? 'narrow',
  )

  // 监听应用主题变化
  useEffect(() => {
    return subscribeToTheme((isDark) => {
      setTheme(isDark ? 'dark' : 'light')
    })
  }, [])

  // 保存设置（主题不保存，每次跟随系统）
  useEffect(() => {
    saveSettings({ fontSize, lineHeight, fontFamily, layout })
  }, [fontSize, lineHeight, fontFamily, layout])

  // 计算值 - useMemo 缓存
  // exlight：chrome 走 glass-solid（100%），不动 standard/light 的 glass-80。
  const currentTheme = useMemo(() => {
    const base = THEMES[theme]
    if (isExlight(anim)) {
      return { ...base, surface: base.surfaceSolid }
    }
    return base
  }, [theme, anim])
  const currentFont = useMemo(
    () => FONT_OPTIONS.find((f) => f.id === fontFamily) || FONT_OPTIONS[0],
    [fontFamily],
  )
  const currentLayout = useMemo(
    () => LAYOUT_OPTIONS.find((l) => l.id === layout) || LAYOUT_OPTIONS[0],
    [layout],
  )
  const isDark = useMemo(() => theme === 'dark' || theme === 'night', [theme])

  // 字体大小调整
  const adjustFontSize = useCallback((delta: number) => {
    setFontSize((prev: number) => Math.max(14, Math.min(28, prev + delta)))
  }, [])

  // 行高调整
  const adjustLineHeight = useCallback((delta: number) => {
    setLineHeight((prev: number) =>
      Math.max(1.4, Math.min(2.4, +(prev + delta).toFixed(1))),
    )
  }, [])

  // 切换主题
  const cycleTheme = useCallback(() => {
    setTheme((prev) => {
      const currentIndex = THEME_ORDER.indexOf(prev)
      return THEME_ORDER[(currentIndex + 1) % THEME_ORDER.length]
    })
  }, [])

  // 切换字体
  const cycleFont = useCallback(() => {
    setFontFamily((prev: string) => {
      const currentIndex = FONT_OPTIONS.findIndex((f) => f.id === prev)
      return FONT_OPTIONS[(currentIndex + 1) % FONT_OPTIONS.length].id
    })
  }, [])

  // 切换布局宽度
  const cycleLayout = useCallback(() => {
    setLayout((prev) => {
      const currentIndex = LAYOUT_OPTIONS.findIndex((l) => l.id === prev)
      return LAYOUT_OPTIONS[(currentIndex + 1) % LAYOUT_OPTIONS.length].id
    })
  }, [])

  return {
    // 状态
    fontSize,
    lineHeight,
    fontFamily,
    theme,
    layout,

    // 计算值
    currentTheme,
    currentFont,
    currentLayout,
    isDark,

    // 操作
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
