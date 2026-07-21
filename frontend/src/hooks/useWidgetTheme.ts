/**
 * 表面主题 Hook —— 全站外观的状态与持久化中枢
 *
 * 两个维度均为「根属性 + CSS 令牌」纯 CSS 驱动，组件零订阅：
 * - 表面（surface）：写 html[data-surface]，theme.css 切换 --surface-* 令牌，
 *   驱动全站 .glass / .glass-surface / 浮动 chrome（玻璃 / 纯色 / 轻盈 / 描边）
 * - 光晕（glow）：写 html[data-glow]，GlowBackground.css 覆盖颜色（primary）
 *   或隐藏（none），默认 identity 用各组件身份色
 *
 * 本 hook 的 React 订阅仅存在于选择器 UI（TitleFontSelector）与全站挂载点
 * （SurfaceThemeApplier，渲染 null）。
 *
 * 持久化：后端 /api/config/dashboard 的 widget_theme 键（JSON 字符串），
 * 与 useTitleFont 同一套「模块级全局状态 + 订阅者 + 防抖保存」范式。
 * 访客通过公开的 /api/config/ui 读到站主配置的主题。
 */

import { useCallback, useEffect, useRef, useState } from 'react'

import { API_URL } from '../config'
import { getUIConfigDeduped } from '../utils/requestDedup'
import { resyncWallpaperBlur } from '../utils/wallpaperState'

// ==================== 类型定义 ====================

export type WidgetSurface = 'glass' | 'solid' | 'flat' | 'outline' | 'liquid'
export type WidgetGlowMode = 'identity' | 'primary' | 'none'

interface WidgetThemeState {
  surface: WidgetSurface
  glow: WidgetGlowMode
}

type WidgetThemeListener = (state: WidgetThemeState) => void

// ==================== 选项配置 ====================

export const SURFACE_OPTIONS: readonly {
  id: WidgetSurface
  nameKey: string
  /** 预览色块类名（静态呈现该质感，见 theme.css .surface-swatch--*） */
  className: string
}[] = Object.freeze([
  { id: 'glass', nameKey: 'surfaceGlass', className: 'surface-swatch--glass' },
  {
    id: 'liquid',
    nameKey: 'surfaceLiquid',
    className: 'surface-swatch--liquid',
  },
  { id: 'solid', nameKey: 'surfaceSolid', className: 'surface-swatch--solid' },
  { id: 'flat', nameKey: 'surfaceFlat', className: 'surface-swatch--flat' },
  {
    id: 'outline',
    nameKey: 'surfaceOutline',
    className: 'surface-swatch--outline',
  },
])

export const GLOW_OPTIONS: readonly {
  id: WidgetGlowMode
  nameKey: string
}[] = Object.freeze([
  { id: 'identity', nameKey: 'glowIdentity' },
  { id: 'primary', nameKey: 'glowPrimary' },
  { id: 'none', nameKey: 'glowNone' },
])

const SURFACE_IDS = new Set<WidgetSurface>(SURFACE_OPTIONS.map((o) => o.id))

/**
 * 把当前主题写到根元素属性，之后全部由 CSS 驱动、无组件订阅：
 * - data-surface → theme.css 切换 --surface-* 令牌，驱动全站 .glass / .glass-surface
 * - data-glow    → GlowBackground.css 覆盖光晕颜色（primary）或隐藏（none）
 * 默认态（glass / identity）不落属性。
 */
function applyThemeToRoot(state: WidgetThemeState): void {
  if (typeof document === 'undefined') return
  const root = document.documentElement
  if (state.surface === 'glass') {
    delete root.dataset.surface
  } else {
    root.dataset.surface = state.surface
  }
  if (state.glow === 'identity') {
    delete root.dataset.glow
  } else {
    root.dataset.glow = state.glow
  }
  // liquid 表面收敛壁纸基础模糊（壁纸清晰、玻璃负责模糊），
  // 切换表面时立即按当前主题重算，不等下一次壁纸应用
  resyncWallpaperBlur()
}

function isSurface(v: unknown): v is WidgetSurface {
  return typeof v === 'string' && SURFACE_IDS.has(v as WidgetSurface)
}

function isGlowMode(v: unknown): v is WidgetGlowMode {
  return v === 'identity' || v === 'primary' || v === 'none'
}

// ==================== 全局状态管理 ====================

const DEFAULT_THEME: WidgetThemeState = { surface: 'glass', glow: 'identity' }

let globalState: WidgetThemeState = { ...DEFAULT_THEME }
const listeners = new Set<WidgetThemeListener>()
let isGlobalInitialized = false
let initPromise: Promise<void> | null = null

function notifyListeners() {
  const state = { ...globalState }
  listeners.forEach((listener) => listener(state))
}

function updateGlobalState(updates: Partial<WidgetThemeState>) {
  const prev = globalState
  globalState = { ...globalState, ...updates }
  if (globalState.surface !== prev.surface || globalState.glow !== prev.glow) {
    applyThemeToRoot(globalState)
  }
  notifyListeners()
}

// 防抖保存（保存完整主题 JSON，避免部分键合并问题）
let saveTimeout: ReturnType<typeof setTimeout> | null = null
const SAVE_DEBOUNCE_MS = 500

function debouncedSave(csrfToken: string) {
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
        body: JSON.stringify({ widget_theme: JSON.stringify(globalState) }),
      })
    } catch (err) {
      console.error('保存小组件主题失败:', err)
    }
  }, SAVE_DEBOUNCE_MS)
}

// 初始化全局状态（读公开 UI 配置，值经白名单校验）
async function initGlobalState(): Promise<void> {
  if (isGlobalInitialized) return
  if (initPromise) return initPromise

  initPromise = (async () => {
    try {
      const data = await getUIConfigDeduped()
      if (data?.widget_theme) {
        try {
          const parsed = JSON.parse(data.widget_theme)
          const updates: Partial<WidgetThemeState> = {}
          if (isSurface(parsed?.surface)) {
            updates.surface = parsed.surface
          }
          if (isGlowMode(parsed?.glow)) {
            updates.glow = parsed.glow
          }
          if (Object.keys(updates).length > 0) {
            updateGlobalState(updates)
          }
        } catch {
          // 配置损坏时保持默认主题
        }
      }
    } catch (err) {
      console.error('加载小组件主题失败:', err)
    } finally {
      isGlobalInitialized = true
      initPromise = null
    }
  })()

  return initPromise
}

// ==================== Hook ====================

export function useWidgetTheme() {
  const [state, setState] = useState<WidgetThemeState>(globalState)
  const mountedRef = useRef(true)

  // 订阅全局状态
  useEffect(() => {
    mountedRef.current = true

    const listener: WidgetThemeListener = (newState) => {
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

  const setSurface = useCallback(
    (surface: WidgetSurface, csrfToken?: string) => {
      if (!isSurface(surface)) return
      updateGlobalState({ surface })
      if (csrfToken) {
        debouncedSave(csrfToken)
      }
    },
    [],
  )

  const setGlowMode = useCallback(
    (glow: WidgetGlowMode, csrfToken?: string) => {
      if (!isGlowMode(glow)) return
      updateGlobalState({ glow })
      if (csrfToken) {
        debouncedSave(csrfToken)
      }
    },
    [],
  )

  return {
    surface: state.surface,
    glow: state.glow,
    setSurface,
    setGlowMode,
  }
}
