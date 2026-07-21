/**
 * 标题字体管理 Hook
 * 按需加载 Astro Fonts API 自托管的标题装饰字体，并管理全局标题字体、大小和颜色设置
 *
 * 性能优化：
 * - 字体懒加载 + 缓存（仅当前/默认字体在 init 时加载；全量预加载仅在选择器打开时）
 * - document.fonts.load 使用各字体实际字重，避免拉错 face
 * - 防抖保存
 * - 全局状态共享避免重复请求
 * - useMemo 缓存计算结果
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { API_URL } from '../config'
import { usePrimaryColor } from '../utils/colorSubscriber'
import { deriveAdaptiveTitleColor } from '../utils/readableColor'
import { getUIConfigDeduped } from '../utils/requestDedup'
import { useThemeMode } from '../utils/themeSubscriber'

// ==================== 类型定义 ====================

export interface FontOption {
  id: string
  name: string
  family: string
  cssVariable: string
  cssClass: string
  /** 实际注册/使用的字重，与 fonts.css .title-font-* 和 astro.config 对齐 */
  weight: 400 | 700
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
// 自适应为默认，放在首位便于发现
export const AVAILABLE_COLORS: readonly ColorOption[] = Object.freeze([
  {
    id: 'adaptive',
    nameKey: 'colorAdaptive',
    value: 'adaptive',
    // 静态兜底：运行时由 getTitleColorCss / deriveAdaptiveTitleColor 按对比度重算
    cssValue: 'var(--color-primary)',
  },
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
])

// 字体大小选项
export const FONT_SIZE_OPTIONS: readonly {
  id: string
  nameKey: string
  value: number
}[] = Object.freeze([
  { id: 'md', nameKey: 'sizeMedium', value: 0.8 },
  { id: 'lg', nameKey: 'sizeLarge', value: 1.0 },
  { id: 'xl', nameKey: 'sizeXLarge', value: 1.2 },
  { id: 'xxl', nameKey: 'sizeXXLarge', value: 1.4 },
])

// 可用字体（weight 与 fonts.css / astro.config 单 face 注册对齐）
export const AVAILABLE_FONTS: readonly FontOption[] = Object.freeze([
  {
    id: 'qwitcher-grypen',
    name: 'Qwitcher Grypen',
    family: 'var(--font-qwitcher-grypen), cursive',
    cssVariable: '--font-qwitcher-grypen',
    cssClass: 'title-font-qwitcher-grypen',
    weight: 700,
  },
  {
    id: 'codystar',
    name: 'Codystar',
    family: 'var(--font-codystar), system-ui',
    cssVariable: '--font-codystar',
    cssClass: 'title-font-codystar',
    weight: 400,
  },
  {
    id: 'henny-penny',
    name: 'Henny Penny',
    family: 'var(--font-henny-penny), system-ui',
    cssVariable: '--font-henny-penny',
    cssClass: 'title-font-henny-penny',
    weight: 400,
  },
  {
    id: 'srisakdi',
    name: 'Srisakdi',
    family: 'var(--font-srisakdi), system-ui',
    cssVariable: '--font-srisakdi',
    cssClass: 'title-font-srisakdi',
    weight: 700,
  },
  {
    id: 'fleur-de-leah',
    name: 'Fleur De Leah',
    family: 'var(--font-fleur-de-leah), cursive',
    cssVariable: '--font-fleur-de-leah',
    cssClass: 'title-font-fleur-de-leah',
    weight: 400,
  },
  {
    id: 'league-script',
    name: 'League Script',
    family: 'var(--font-league-script), cursive',
    cssVariable: '--font-league-script',
    cssClass: 'title-font-league-script',
    weight: 400,
  },
  {
    id: 'megrim',
    name: 'Megrim',
    family: 'var(--font-megrim), system-ui',
    cssVariable: '--font-megrim',
    cssClass: 'title-font-megrim',
    weight: 400,
  },
  {
    id: 'silkscreen',
    name: 'Silkscreen',
    family: 'var(--font-silkscreen), system-ui',
    cssVariable: '--font-silkscreen',
    cssClass: 'title-font-silkscreen',
    weight: 700,
  },
  {
    id: 'unifraktur-maguntia',
    name: 'UnifrakturMaguntia',
    family: 'var(--font-unifraktur-maguntia), serif',
    cssVariable: '--font-unifraktur-maguntia',
    cssClass: 'title-font-unifraktur-maguntia',
    weight: 400,
  },
  {
    id: 'cinzel',
    name: 'Cinzel',
    family: 'var(--font-cinzel), serif',
    cssVariable: '--font-cinzel',
    cssClass: 'title-font-cinzel',
    weight: 700,
  },
])

// 创建快速查找 Map
const fontMap = new Map(AVAILABLE_FONTS.map((f) => [f.id, f]))
const colorMap = new Map(AVAILABLE_COLORS.map((c) => [c.id, c]))
const sizeMap = new Map(FONT_SIZE_OPTIONS.map((s) => [s.value, s]))

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

  // Astro Fonts API 已在构建时声明 @font-face 并自托管；按实际字重触发下载
  // 从 CSS 变量读取实际的哈希字体名，用 document.fonts.load() 触发浏览器下载
  const computedValue = getComputedStyle(document.documentElement)
    .getPropertyValue(font.cssVariable)
    .trim()
  // CSS 变量值可能包含 fallback 列表（如 "Inter-hash, -apple-system, ..."），
  // document.fonts.load() 仅需第一个字体名
  const primaryFamily = computedValue
    ? computedValue
        .split(',')[0]
        .trim()
        .replace(/^["']|["']$/g, '')
    : font.name

  // 带字重加载，确保拉取与 @font-face / hero 使用一致的 face
  const promise = document.fonts
    .load(`${font.weight} 16px "${primaryFamily}"`)
    .then(() => {
      loadedFonts.add(font.id)
    })
    .catch(() => {
      console.warn(`Failed to load font: ${font.name}`)
    })
    .finally(() => {
      loadingFonts.delete(font.id)
    })

  loadingFonts.set(font.id, promise)
  return promise
}

// ==================== 全局状态管理 ====================

let globalState: TitleStyle = {
  font: 'qwitcher-grypen',
  fontSize: 1.0,
  // 默认自适应：随主题/壁纸色对比度推导可读标题色
  color: 'adaptive',
}

const listeners = new Set<TitleStyleListener>()
let isGlobalInitialized = false
let initPromise: Promise<void> | null = null

function notifyListeners() {
  const state = { ...globalState }
  listeners.forEach((listener) => listener(state))
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
  settings: Partial<{
    title_font: string
    title_font_size: number
    title_color: string
  }>,
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
    } catch (err) {
      console.error('保存标题样式失败:', err)
    }
  }, SAVE_DEBOUNCE_MS)
}

// 初始化全局状态
async function initGlobalState(): Promise<void> {
  if (isGlobalInitialized) return
  if (initPromise) return initPromise

  initPromise = (async () => {
    try {
      const data = await getUIConfigDeduped()
      if (!data) return

      // 批量更新状态
      const updates: Partial<TitleStyle> = {}

      if (data.title_font && fontMap.has(data.title_font)) {
        updates.font = data.title_font
        const font = fontMap.get(data.title_font)
        if (font) loadFont(font) // 异步加载，不阻塞
      }

      if (data.title_font_size != null) {
        const fontSize = Number(data.title_font_size)
        if (!Number.isNaN(fontSize) && sizeMap.has(fontSize)) {
          updates.fontSize = fontSize
        }
      }

      if (data.title_color && colorMap.has(data.title_color)) {
        updates.color = data.title_color
      }

      if (Object.keys(updates).length > 0) {
        updateGlobalState(updates)
      }
    } catch (err) {
      console.error('加载标题样式设置失败:', err)
    } finally {
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
  const currentFont = useMemo(
    () => fontMap.get(state.font) || AVAILABLE_FONTS[0],
    [state.font],
  )
  const currentColor = useMemo(
    () => colorMap.get(state.color) || AVAILABLE_COLORS[0],
    [state.color],
  )
  const currentFontSizeOption = useMemo(
    () => sizeMap.get(state.fontSize) || FONT_SIZE_OPTIONS[1],
    [state.fontSize],
  )

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
  const setTitleFont = useCallback(
    async (fontId: string, csrfToken?: string) => {
      const font = fontMap.get(fontId)
      if (!font) return

      setIsLoading(true)
      try {
        await loadFont(font)
        updateGlobalState({ font: fontId })
        if (csrfToken) {
          debouncedSave(csrfToken, { title_font: fontId })
        }
      } finally {
        if (mountedRef.current) {
          setIsLoading(false)
        }
      }
    },
    [],
  )

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

/**
 * 解析标题颜色 CSS 值。
 * adaptive：对齐 Tapp 音乐播放器歌词填色 —— 基于 WCAG 对比度在主题背景下推导可读色。
 */
export function getTitleColorCss(colorId?: string, isDark?: boolean): string {
  const id = colorId || globalState.color
  const color = colorMap.get(id)
  if (!color) return AVAILABLE_COLORS[0].cssValue

  if (id === 'adaptive') {
    const dark =
      isDark ??
      (typeof document !== 'undefined'
        ? document.documentElement.classList.contains('dark')
        : false)
    return deriveAdaptiveTitleColor(dark)
  }

  return color.cssValue
}

/**
 * 响应式标题色：跟随标题色设置、明暗主题、壁纸主色变化自动重算。
 * 页面 Hero 统一用此 hook，避免各处重复 MutationObserver + adaptive 逻辑。
 */
export function useResolvedTitleColor(
  colorType: 'primary' | 'accent' = 'primary',
): string {
  const { titleColor } = useTitleFont()
  const isDark = useThemeMode()
  // 壁纸取色变更时触发重算（adaptive 依赖 --color-*）
  const primaryColor = usePrimaryColor()

  return useMemo(() => {
    // primaryColor 作为壁纸色指纹：CSS 变量更新时强制重算 adaptive
    void primaryColor
    // Reports 默认双色：仅当用户未改标题色时，第二标题可用 accent
    if (titleColor === 'primary' && colorType === 'accent') {
      return 'color-mix(in srgb, var(--color-accent) 70%, transparent)'
    }
    return getTitleColorCss(titleColor, isDark)
  }, [titleColor, isDark, primaryColor, colorType])
}
