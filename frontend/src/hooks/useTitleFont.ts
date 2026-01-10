/**
 * 标题字体管理 Hook
 * 动态按需加载 Google Fonts 并管理全局标题字体、大小和颜色设置
 *
 * 性能优化：
 * - 字体懒加载 + 缓存
 * - 防抖保存
 * - 全局状态共享避免重复请求
 * - useMemo 缓存计算结果
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { API_URL } from '../config'

// ==================== 类型定义 ====================

export interface FontOption {
  id: string
  name: string
  family: string
  googleUrl: string
  cssClass: string
}

export interface ColorOption {
  id: string
  nameKey: string // i18n key for name
  value: string
  cssValue: string
}

interface TitleStyle {
  font: string
  fontSize: number
  color: string
}

type TitleStyleListener = (style: TitleStyle) => void

// ==================== 常量配置 ====================

// 颜色选项（基于全局壁纸色变量）
export const AVAILABLE_COLORS: readonly ColorOption[] = Object.freeze([
  {
    id: 'primary',
    nameKey: 'colorPrimary',
    value: 'var(--color-primary)',
    cssValue: 'color-mix(in srgb, var(--color-primary) 70%, transparent)',
  },
  {
    id: 'secondary',
    nameKey: 'colorSecondary',
    value: 'var(--color-secondary)',
    cssValue: 'color-mix(in srgb, var(--color-secondary) 70%, transparent)',
  },
  {
    id: 'accent',
    nameKey: 'colorAccent',
    value: 'var(--color-accent)',
    cssValue: 'color-mix(in srgb, var(--color-accent) 70%, transparent)',
  },
  {
    id: 'light',
    nameKey: 'colorLight',
    value: 'var(--color-light)',
    cssValue: 'color-mix(in srgb, var(--color-light) 70%, transparent)',
  },
  {
    id: 'dark',
    nameKey: 'colorDark',
    value: 'var(--color-dark)',
    cssValue: 'color-mix(in srgb, var(--color-dark) 70%, transparent)',
  },
  {
    id: 'adaptive',
    nameKey: 'colorAdaptive',
    value: 'adaptive',
    cssValue: 'color-mix(in srgb, var(--color-primary) 50%, var(--adaptive-base))',
  },
])

// 字体大小选项
export const FONT_SIZE_OPTIONS: readonly { id: string, nameKey: string, value: number }[] = Object.freeze([
  { id: 'md', nameKey: 'sizeMedium', value: 0.8 },
  { id: 'lg', nameKey: 'sizeLarge', value: 1.0 },
  { id: 'xl', nameKey: 'sizeXLarge', value: 1.2 },
  { id: 'xxl', nameKey: 'sizeXXLarge', value: 1.4 },
])

// 可用字体
export const AVAILABLE_FONTS: readonly FontOption[] = Object.freeze([
  {
    id: 'qwitcher-grypen',
    name: 'Qwitcher Grypen',
    family: '"Qwitcher Grypen", cursive',
    googleUrl: 'https://fonts.googleapis.com/css2?family=Qwitcher+Grypen:wght@700&display=swap',
    cssClass: 'title-font-qwitcher-grypen',
  },
  {
    id: 'codystar',
    name: 'Codystar',
    family: '"Codystar", system-ui',
    googleUrl: 'https://fonts.googleapis.com/css2?family=Codystar&display=swap',
    cssClass: 'title-font-codystar',
  },
  {
    id: 'henny-penny',
    name: 'Henny Penny',
    family: '"Henny Penny", system-ui',
    googleUrl: 'https://fonts.googleapis.com/css2?family=Henny+Penny&display=swap',
    cssClass: 'title-font-henny-penny',
  },
  {
    id: 'srisakdi',
    name: 'Srisakdi',
    family: '"Srisakdi", system-ui',
    googleUrl: 'https://fonts.googleapis.com/css2?family=Srisakdi:wght@700&display=swap',
    cssClass: 'title-font-srisakdi',
  },
  {
    id: 'fleur-de-leah',
    name: 'Fleur De Leah',
    family: '"Fleur De Leah", cursive',
    googleUrl: 'https://fonts.googleapis.com/css2?family=Fleur+De+Leah&display=swap',
    cssClass: 'title-font-fleur-de-leah',
  },
  {
    id: 'league-script',
    name: 'League Script',
    family: '"League Script", cursive',
    googleUrl: 'https://fonts.googleapis.com/css2?family=League+Script&display=swap',
    cssClass: 'title-font-league-script',
  },
  {
    id: 'megrim',
    name: 'Megrim',
    family: '"Megrim", system-ui',
    googleUrl: 'https://fonts.googleapis.com/css2?family=Megrim&display=swap',
    cssClass: 'title-font-megrim',
  },
  {
    id: 'silkscreen',
    name: 'Silkscreen',
    family: '"Silkscreen", system-ui',
    googleUrl: 'https://fonts.googleapis.com/css2?family=Silkscreen:wght@700&display=swap',
    cssClass: 'title-font-silkscreen',
  },
  {
    id: 'unifraktur-maguntia',
    name: 'UnifrakturMaguntia',
    family: '"UnifrakturMaguntia", serif',
    googleUrl: 'https://fonts.googleapis.com/css2?family=UnifrakturMaguntia&display=swap',
    cssClass: 'title-font-unifraktur-maguntia',
  },
  {
    id: 'cinzel',
    name: 'Cinzel',
    family: '"Cinzel", serif',
    googleUrl: 'https://fonts.googleapis.com/css2?family=Cinzel:wght@700&display=swap',
    cssClass: 'title-font-cinzel',
  },
])

// 创建快速查找 Map
const fontMap = new Map(AVAILABLE_FONTS.map(f => [f.id, f]))
const colorMap = new Map(AVAILABLE_COLORS.map(c => [c.id, c]))
const sizeMap = new Map(FONT_SIZE_OPTIONS.map(s => [s.value, s]))

// ==================== 字体加载器 ====================

const loadedFonts = new Set<string>()
const loadingFonts = new Map<string, Promise<void>>()

function loadFont(font: FontOption): Promise<void> {
  // 已加载
  if (loadedFonts.has(font.id)) {
    return Promise.resolve()
  }

  // 正在加载，返回现有 Promise
  const existing = loadingFonts.get(font.id)
  if (existing) {
    return existing
  }

  // 创建新的加载 Promise
  const promise = new Promise<void>((resolve) => {
    const link = document.createElement('link')
    link.rel = 'stylesheet'
    link.href = font.googleUrl

    const cleanup = () => {
      loadingFonts.delete(font.id)
    }

    link.onload = () => {
      loadedFonts.add(font.id)
      cleanup()
      resolve()
    }

    link.onerror = () => {
      console.warn(`Failed to load font: ${font.name}`)
      cleanup()
      resolve() // 即使失败也继续
    }

    document.head.appendChild(link)
  })

  loadingFonts.set(font.id, promise)
  return promise
}

// ==================== 全局状态管理 ====================

let globalState: TitleStyle = {
  font: 'qwitcher-grypen',
  fontSize: 1.0,
  color: 'primary',
}

const listeners = new Set<TitleStyleListener>()
let isGlobalInitialized = false
let initPromise: Promise<void> | null = null

function notifyListeners() {
  const state = { ...globalState }
  listeners.forEach(listener => listener(state))
}

function updateGlobalState(updates: Partial<TitleStyle>) {
  globalState = { ...globalState, ...updates }
  notifyListeners()
}

// 防抖保存
let saveTimeout: ReturnType<typeof setTimeout> | null = null
const SAVE_DEBOUNCE_MS = 500

async function debouncedSave(
  csrfToken: string,
  settings: Partial<{ title_font: string, title_font_size: number, title_color: string }>,
) {
  if (saveTimeout) {
    clearTimeout(saveTimeout)
  }

  saveTimeout = setTimeout(async () => {
    try {
      await fetch(`${API_URL}/api/config/dashboard`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-CSRF-Token': csrfToken,
        },
        credentials: 'include',
        body: JSON.stringify(settings),
      })
    }
    catch (err) {
      console.error('保存标题样式失败:', err)
    }
  }, SAVE_DEBOUNCE_MS)
}

// 初始化全局状态
async function initGlobalState(): Promise<void> {
  if (isGlobalInitialized)
    return
  if (initPromise)
    return initPromise

  initPromise = (async () => {
    try {
      const response = await fetch(`${API_URL}/api/config/ui`)
      if (!response.ok)
        return

      const data = await response.json()

      // 批量更新状态
      const updates: Partial<TitleStyle> = {}

      if (data.title_font && fontMap.has(data.title_font)) {
        updates.font = data.title_font
        const font = fontMap.get(data.title_font)
        if (font)
          loadFont(font) // 异步加载，不阻塞
      }

      if (data.title_font_size != null) {
        const fontSize = Number(data.title_font_size)
        if (!isNaN(fontSize) && sizeMap.has(fontSize)) {
          updates.fontSize = fontSize
        }
      }

      if (data.title_color && colorMap.has(data.title_color)) {
        updates.color = data.title_color
      }

      if (Object.keys(updates).length > 0) {
        updateGlobalState(updates)
      }
    }
    catch (err) {
      console.error('加载标题样式设置失败:', err)
    }
    finally {
      isGlobalInitialized = true
      initPromise = null
    }
  })()

  // 预加载默认字体
  const defaultFont = AVAILABLE_FONTS[0]
  loadFont(defaultFont)

  return initPromise
}

// ==================== Hook ====================

export function useTitleFont() {
  const [state, setState] = useState<TitleStyle>(globalState)
  const [isLoading, setIsLoading] = useState(false)
  const mountedRef = useRef(true)

  // 缓存当前配置
  const currentFont = useMemo(() => fontMap.get(state.font) || AVAILABLE_FONTS[0], [state.font])
  const currentColor = useMemo(() => colorMap.get(state.color) || AVAILABLE_COLORS[0], [state.color])
  const currentFontSizeOption = useMemo(() => sizeMap.get(state.fontSize) || FONT_SIZE_OPTIONS[1], [state.fontSize])

  // 订阅全局状态
  useEffect(() => {
    mountedRef.current = true

    const listener: TitleStyleListener = (newState) => {
      if (mountedRef.current) {
        setState(newState)
      }
    }

    listeners.add(listener)
    initGlobalState() // 触发初始化

    return () => {
      mountedRef.current = false
      listeners.delete(listener)
    }
  }, [])

  // 设置字体
  const setTitleFont = useCallback(async (fontId: string, csrfToken?: string) => {
    const font = fontMap.get(fontId)
    if (!font)
      return

    setIsLoading(true)
    try {
      await loadFont(font)
      updateGlobalState({ font: fontId })
      if (csrfToken) {
        debouncedSave(csrfToken, { title_font: fontId })
      }
    }
    finally {
      if (mountedRef.current) {
        setIsLoading(false)
      }
    }
  }, [])

  // 设置字体大小
  const setTitleFontSize = useCallback((size: number, csrfToken?: string) => {
    updateGlobalState({ fontSize: size })
    if (csrfToken) {
      debouncedSave(csrfToken, { title_font_size: size })
    }
  }, [])

  // 设置颜色
  const setTitleColor = useCallback((colorId: string, csrfToken?: string) => {
    updateGlobalState({ color: colorId })
    if (csrfToken) {
      debouncedSave(csrfToken, { title_color: colorId })
    }
  }, [])

  // 预加载所有字体
  const preloadAllFonts = useCallback(() => {
    return Promise.all(AVAILABLE_FONTS.map(loadFont))
  }, [])

  return {
    titleFont: state.font,
    titleFontSize: state.fontSize,
    titleColor: state.color,
    currentFont,
    currentColor,
    currentFontSizeOption,
    setTitleFont,
    setTitleFontSize,
    setTitleColor,
    isLoading,
    availableFonts: AVAILABLE_FONTS,
    availableColors: AVAILABLE_COLORS,
    fontSizeOptions: FONT_SIZE_OPTIONS,
    preloadAllFonts,
  }
}

// ==================== 工具函数 ====================

export function getTitleFontFamily(fontId?: string): string {
  const id = fontId || globalState.font
  return fontMap.get(id)?.family || AVAILABLE_FONTS[0].family
}

export function getCurrentTitleFontId(): string {
  return globalState.font
}

export function getCurrentTitleFontSize(): number {
  return globalState.fontSize
}

export function getCurrentTitleColorId(): string {
  return globalState.color
}

export function getTitleColorCss(colorId?: string, isDark?: boolean): string {
  const id = colorId || globalState.color
  const color = colorMap.get(id)
  if (!color)
    return AVAILABLE_COLORS[0].cssValue

  if (id === 'adaptive') {
    const adaptiveBase = isDark ? '#ffffff' : '#000000'
    return `color-mix(in srgb, var(--color-primary) 50%, ${adaptiveBase})`
  }

  return color.cssValue
}
