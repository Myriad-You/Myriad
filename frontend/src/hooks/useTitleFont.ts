import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { API_URL } from '../config'
import { currentCopy } from '../i18n/localeCopy'
import { SITE_TITLE_FONTS } from '../siteFonts.mjs'
import { usePrimaryColor } from '../utils/colorSubscriber'
import { formatUserFacingError } from '../utils/formatUserFacingError'
import { deriveAdaptiveTitleColor } from '../utils/readableColor'
import { getUIConfigDeduped } from '../utils/requestDedup'
import { useThemeMode } from '../utils/themeSubscriber'
import { showError } from '../utils/toastManager'

export interface FontOption {
  id: string
  name: string
  family: string
  cssVariable: string
  cssClass: string

  /** 实际注册/使用的字重，与 fonts.css / siteFonts 对齐。 */
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

export const AVAILABLE_COLORS: readonly ColorOption[] = Object.freeze([
  {
    id: 'adaptive',
    nameKey: 'colorAdaptive',
    value: 'adaptive',

    // 静态兜底：运行时按对比度重算。
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

export const AVAILABLE_FONTS: readonly FontOption[] = Object.freeze(
  SITE_TITLE_FONTS.map((font) => ({
    id: font.id,
    name: font.name,
    family: `var(${font.cssVariable}), ${font.fallbacks[0]}`,
    cssVariable: font.cssVariable,
    cssClass: font.cssClass,
    weight: font.weights[0] as 400 | 700,
  })),
)

const fontMap = new Map(AVAILABLE_FONTS.map((f) => [f.id, f]))
const colorMap = new Map(AVAILABLE_COLORS.map((c) => [c.id, c]))
const sizeMap = new Map(FONT_SIZE_OPTIONS.map((s) => [s.value, s]))

const loadedFonts = new Set<string>()
const loadingFonts = new Map<string, Promise<void>>()

function loadFont(font: FontOption): Promise<void> {
  if (loadedFonts.has(font.id)) {
    return Promise.resolve()
  }

  // 正在加载，返回现有 Promise。
  const existing = loadingFonts.get(font.id)
  if (existing) {
    return existing
  }

  // CSS 变量可能含 fallback 列表；fonts.load 只要第一个字体名。
  const computedValue = getComputedStyle(document.documentElement)
    .getPropertyValue(font.cssVariable)
    .trim()

  const primaryFamily = computedValue
    ? computedValue
        .split(',')[0]
        .trim()
        .replaceAll(/^["']|["']$/g, '')
    : font.name

  // 带字重加载，确保拉取与 @font-face / hero 使用一致的 face。
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

let globalState: TitleStyle = {
  font: 'qwitcher-grypen',
  fontSize: 1.0,

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

let saveTimeout: ReturnType<typeof setTimeout> | null = null
const SAVE_DEBOUNCE_MS = 500
let pendingSave: Partial<{
  title_font: string
  title_font_size: number
  title_color: string
}> = {}
let pendingCsrfToken = ''

async function debouncedSave(
  csrfToken: string,
  settings: Partial<{
    title_font: string
    title_font_size: number
    title_color: string
  }>,
) {
  pendingSave = { ...pendingSave, ...settings }
  pendingCsrfToken = csrfToken || pendingCsrfToken

  if (saveTimeout) {
    clearTimeout(saveTimeout)
  }

  saveTimeout = setTimeout(async () => {
    const payload = { ...pendingSave }
    const token = pendingCsrfToken
    pendingSave = {}
    pendingCsrfToken = ''
    saveTimeout = null
    if (Object.keys(payload).length === 0) return
    try {
      const res = await fetch(`${API_URL}/api/config/dashboard`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-CSRF-Token': token,
        },
        credentials: 'include',
        body: JSON.stringify(payload),
      })
      if (!res.ok) {
        throw new Error(`Failed to save title style: HTTP ${res.status}`)
      }
    } catch (err) {
      console.error('保存标题样式失败:', err)
      showError(
        await formatUserFacingError(
          err,
          currentCopy().errors.titleStyleSaveFailed,
        ),
      )
    }
  }, SAVE_DEBOUNCE_MS)
}

async function initGlobalState(): Promise<void> {
  if (isGlobalInitialized) return
  if (initPromise) return initPromise

  initPromise = (async () => {
    try {
      const data = await getUIConfigDeduped()
      if (!data) return

      const updates: Partial<TitleStyle> = {}

      if (data.title_font && fontMap.has(data.title_font)) {
        updates.font = data.title_font
        const font = fontMap.get(data.title_font)
        if (font) loadFont(font)
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

  const defaultFont = AVAILABLE_FONTS[0]
  loadFont(defaultFont)

  return initPromise
}

export function useTitleFont() {
  const [state, setState] = useState<TitleStyle>(globalState)
  const [isLoading, setIsLoading] = useState(false)
  const mountedRef = useRef(true)

  const currentFont = useMemo(
    () => fontMap.get(state.font) ?? AVAILABLE_FONTS[0],
    [state.font],
  )
  const currentColor = useMemo(
    () => colorMap.get(state.color) ?? AVAILABLE_COLORS[0],
    [state.color],
  )
  const currentFontSizeOption = useMemo(
    () => sizeMap.get(state.fontSize) ?? FONT_SIZE_OPTIONS[1],
    [state.fontSize],
  )

  useEffect(() => {
    mountedRef.current = true

    const listener: TitleStyleListener = (newState) => {
      if (mountedRef.current) {
        setState(newState)
      }
    }

    listeners.add(listener)
    initGlobalState()

    return () => {
      mountedRef.current = false
      listeners.delete(listener)
    }
  }, [])

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

  const setTitleFontSize = useCallback((size: number, csrfToken?: string) => {
    updateGlobalState({ fontSize: size })
    if (csrfToken) {
      debouncedSave(csrfToken, { title_font_size: size })
    }
  }, [])

  const setTitleColor = useCallback((colorId: string, csrfToken?: string) => {
    updateGlobalState({ color: colorId })
    if (csrfToken) {
      debouncedSave(csrfToken, { title_color: colorId })
    }
  }, [])

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

/** adaptive：按 WCAG 对比度在主题背景下推导可读色。 */
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

export function useResolvedTitleColor(
  colorType: 'primary' | 'accent' = 'primary',
): string {
  const { titleColor } = useTitleFont()
  const isDark = useThemeMode()

  const primaryColor = usePrimaryColor()

  return useMemo(() => {
    // primaryColor 作壁纸色指纹：CSS 变量更新时强制重算 adaptive。
    void primaryColor

    // Reports 默认双色：仅当用户未改标题色时，第二标题可用 accent。
    if (titleColor === 'primary' && colorType === 'accent') {
      return 'color-mix(in srgb, var(--color-accent) 70%, transparent)'
    }
    return getTitleColorCss(titleColor, isDark)
  }, [titleColor, isDark, primaryColor, colorType])
}
