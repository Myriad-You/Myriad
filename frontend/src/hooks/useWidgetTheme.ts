import { useCallback, useEffect, useRef, useState } from 'react'

import { API_URL } from '../config'
import { currentCopy } from '../i18n/localeCopy'
import { formatUserFacingError } from '../utils/formatUserFacingError'
import { getUIConfigDeduped } from '../utils/requestDedup'
import { showError } from '../utils/toastManager'
import { resyncWallpaperBlur } from '../utils/wallpaperState'

export type WidgetSurface = 'glass' | 'solid' | 'flat' | 'outline' | 'liquid'
export type WidgetGlowMode = 'identity' | 'primary' | 'none'

interface WidgetThemeState {
  surface: WidgetSurface
  glow: WidgetGlowMode
}

type WidgetThemeListener = (state: WidgetThemeState) => void

export const SURFACE_OPTIONS: readonly {
  id: WidgetSurface
  nameKey: string

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

/** 默认态（glass / identity）不落属性。 */
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

  // liquid 表面收敛壁纸基础模糊；切表面时立刻按当前主题重算。
  resyncWallpaperBlur()
}

function isSurface(v: unknown): v is WidgetSurface {
  return typeof v === 'string' && SURFACE_IDS.has(v as WidgetSurface)
}

function isGlowMode(v: unknown): v is WidgetGlowMode {
  return v === 'identity' || v === 'primary' || v === 'none'
}

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

let saveTimeout: ReturnType<typeof setTimeout> | null = null
const SAVE_DEBOUNCE_MS = 500

// 保存完整主题 JSON，避免部分键合并问题。
function debouncedSave(csrfToken: string) {
  if (saveTimeout) {
    clearTimeout(saveTimeout)
  }

  saveTimeout = setTimeout(async () => {
    try {
      const res = await fetch(`${API_URL}/api/config/dashboard`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-CSRF-Token': csrfToken,
        },
        credentials: 'include',
        body: JSON.stringify({ widget_theme: JSON.stringify(globalState) }),
      })
      if (!res.ok) {
        throw new Error(`Failed to save widget theme: HTTP ${res.status}`)
      }
    } catch (err) {
      console.error('保存小组件主题失败:', err)
      showError(
        await formatUserFacingError(
          err,
          currentCopy().errors.widgetThemeSaveFailed,
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
          // 配置损坏时保持默认主题。
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

export function useWidgetTheme() {
  const [state, setState] = useState<WidgetThemeState>(globalState)
  const mountedRef = useRef(true)

  useEffect(() => {
    mountedRef.current = true

    const listener: WidgetThemeListener = (newState) => {
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
