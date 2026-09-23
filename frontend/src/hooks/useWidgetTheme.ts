import { useCallback, useEffect, useSyncExternalStore } from 'react'

import { authSubject } from '../utils/authSubject'
import { getUIConfigDeduped } from '../utils/requestDedup'
import { resyncWallpaperBlur } from '../utils/wallpaperState'

export type WidgetSurface = 'glass' | 'solid' | 'flat' | 'outline' | 'liquid'
export type WidgetGlowMode = 'identity' | 'primary' | 'none'

interface WidgetThemeState {
  surface: WidgetSurface
  glow: WidgetGlowMode
}

type WidgetThemeListener = () => void

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

let globalState = DEFAULT_THEME
const edited = new Set<keyof WidgetThemeState>()
const listeners = new Set<WidgetThemeListener>()
let isGlobalInitialized = false
let initPromise: Promise<void> | null = null

function subscribeTheme(listener: WidgetThemeListener) {
  listeners.add(listener)
  return () => { listeners.delete(listener) }
}

const getTheme = () => globalState
const getServerTheme = () => DEFAULT_THEME

function updateGlobalState(updates: Partial<WidgetThemeState>) {
  const next = { ...globalState, ...updates }
  if (next.surface === globalState.surface && next.glow === globalState.glow) return
  globalState = next
  applyThemeToRoot(globalState)
  listeners.forEach(listener => listener())
}

async function persistTheme() {
  const owner = authSubject.signal
  // The endpoint replaces the entire theme. Resolve untouched server fields
  // before taking the snapshot, while local preview remains immediate.
  await initGlobalState()
  if (owner.aborted) return
  const widget_theme = JSON.stringify(globalState)
  const { saveDashboardAppearance } = await import('../services/dashboardAppearancePersistence')
  saveDashboardAppearance({ widget_theme }, 'widgetThemeSaveFailed', owner)
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
          if (!edited.has('surface') && isSurface(parsed?.surface)) {
            updates.surface = parsed.surface
          }
          if (!edited.has('glow') && isGlowMode(parsed?.glow)) {
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
  const state = useSyncExternalStore(subscribeTheme, getTheme, getServerTheme)
  useEffect(() => { void initGlobalState() }, [])

  const setSurface = useCallback(
    (surface: WidgetSurface) => {
      if (!isSurface(surface)) return
      edited.add('surface')
      updateGlobalState({ surface })
      void persistTheme()
    },
    [],
  )

  const setGlowMode = useCallback(
    (glow: WidgetGlowMode) => {
      if (!isGlowMode(glow)) return
      edited.add('glow')
      updateGlobalState({ glow })
      void persistTheme()
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
