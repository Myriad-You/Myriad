/**
 * Navigation island layout resolver.
 *
 * Two chrome modes:
 * - `mobile`  — bottom horizontal island (phones + touch tablets in the mid band)
 * - `desktop` — left vertical rail (wide viewports + pointer/keyboard desktops)
 *
 * Pure width breakpoints mis-classify tablets as desktop rail. We refine the
 * tablet band (768 … desktopMin-1, desktopMin from viewportBands = 1078) with
 * pointer / Apple-touch signals so rotation and docked keyboards stay stable.
 */

import {
  isAppleTouchDevice,
  isCoarsePointerPrimary,
} from './platformDetect'
import {
  VIEWPORT_DESKTOP_MIN,
  VIEWPORT_PHONE_MAX,
} from './viewportBands'

export type NavLayout = 'mobile' | 'desktop'

/** Match Tailwind `md` / viewportBands phone edge. */
export const NAV_MOBILE_MAX_WIDTH = VIEWPORT_PHONE_MAX
/** ≥ this is always desktop rail (viewportBands desktop min). */
export const NAV_DESKTOP_MIN_WIDTH = VIEWPORT_DESKTOP_MIN

export interface NavLayoutSignals {
  width: number
  coarsePointer: boolean
  appleTouch: boolean
}

/**
 * Pure resolver (testable). Prefer {@link getNavLayout} in app code.
 *
 * Rules:
 * 1. width ≤ 767 → mobile
 * 2. width ≥ VIEWPORT_DESKTOP_MIN (1078) → desktop
 * 3. tablet band in between:
 *    - coarse pointer or Apple touch → mobile (iPad portrait, Android tablets)
 *    - fine pointer + hover capable → desktop (narrow desktop window)
 */
export function resolveNavLayout(signals: NavLayoutSignals): NavLayout {
  const width = Number.isFinite(signals.width) ? signals.width : 0
  if (width <= NAV_MOBILE_MAX_WIDTH) return 'mobile'
  if (width >= NAV_DESKTOP_MIN_WIDTH) return 'desktop'
  if (signals.coarsePointer || signals.appleTouch) return 'mobile'
  return 'desktop'
}

export function readNavLayoutSignals(
  win: Window = typeof window !== 'undefined' ? window : (undefined as never),
): NavLayoutSignals {
  if (typeof win === 'undefined' || !win) {
    return { width: NAV_DESKTOP_MIN_WIDTH, coarsePointer: false, appleTouch: false }
  }
  return {
    width: win.innerWidth,
    coarsePointer: isCoarsePointerPrimary(),
    appleTouch: isAppleTouchDevice(),
  }
}

/** Current nav layout for this viewport / input class. */
export function getNavLayout(
  win?: Window,
): NavLayout {
  if (typeof window === 'undefined' && !win) return 'desktop'
  return resolveNavLayout(readNavLayoutSignals(win ?? window))
}

export function isDesktopNavLayout(win?: Window): boolean {
  return getNavLayout(win) === 'desktop'
}

export function isMobileNavLayout(win?: Window): boolean {
  return getNavLayout(win) === 'mobile'
}

/**
 * Sync layout token onto <html> so CSS can key off it without JS class thrash.
 *  dataset.navLayout  ↔  data-nav-layout
 */
export function applyNavLayoutToDocument(
  layout: NavLayout,
  doc: Document = typeof document !== 'undefined' ? document : (undefined as never),
): void {
  if (!doc?.documentElement) return
  if (doc.documentElement.dataset.navLayout === layout) return
  doc.documentElement.dataset.navLayout = layout
}

type Listener = () => void

const listeners = new Set<Listener>()
let subscribed = false
let cachedLayout: NavLayout | null = null
let mqWidth: MediaQueryList | null = null
let mqPointer: MediaQueryList | null = null

/** Resize debounce — avoid thrashing mid-drag across 768/1024. */
const LAYOUT_RESIZE_DEBOUNCE_MS = 48

/**
 * Document `data-nav-layout` is applied by NavigationIsland after chrome
 * crossfade (not here on every resize). First subscribe still seeds FOUC.
 */
function recomputeAndNotify(): void {
  const next = getNavLayout()
  if (cachedLayout === next) return
  cachedLayout = next
  listeners.forEach((l) => l())
}

function ensureSubscribed(): void {
  if (subscribed || typeof window === 'undefined') return
  subscribed = true
  cachedLayout = getNavLayout()
  applyNavLayoutToDocument(cachedLayout)

  let resizeTimer: ReturnType<typeof setTimeout> | null = null
  const onResize = () => {
    if (resizeTimer) clearTimeout(resizeTimer)
    resizeTimer = setTimeout(() => {
      resizeTimer = null
      recomputeAndNotify()
    }, LAYOUT_RESIZE_DEBOUNCE_MS)
  }
  // Orientation: apply on next frame (no long debounce)
  const onOrientation = () => {
    if (resizeTimer) clearTimeout(resizeTimer)
    resizeTimer = null
    requestAnimationFrame(() => recomputeAndNotify())
  }

  window.addEventListener('resize', onResize, { passive: true })
  window.addEventListener('orientationchange', onOrientation, { passive: true })

  try {
    // Fire when crossing phone / tablet / desktop bands without waiting for
    // every pixel of resize (cheaper than raw resize alone on some engines).
    mqWidth = window.matchMedia(
      `(max-width: ${NAV_MOBILE_MAX_WIDTH}px), (min-width: ${NAV_DESKTOP_MIN_WIDTH}px)`,
    )
    mqWidth.addEventListener('change', onResize)
  } catch {
    mqWidth = null
  }
  try {
    mqPointer = window.matchMedia('(hover: none) and (pointer: coarse)')
    mqPointer.addEventListener('change', onResize)
  } catch {
    mqPointer = null
  }
}

/** Fired on `.nav-container` after bottom↔rail chrome crossfade settles. */
export const NAV_CHROME_SETTLED_EVENT = 'navChromeSettled'

/** data-nav-switch values while island crossfades between layouts */
export type NavChromeSwitchPhase = 'out' | 'in'

/**
 * Subscribe to nav layout changes (resize, orientation, pointer class).
 * Returns unsubscribe. Safe to call on the server (no-op).
 */
export function subscribeNavLayout(listener: Listener): () => void {
  if (typeof window === 'undefined') return () => {}
  ensureSubscribed()
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

/** Snapshot for useSyncExternalStore. */
export function getNavLayoutSnapshot(): NavLayout {
  if (typeof window === 'undefined') return 'desktop'
  ensureSubscribed()
  return cachedLayout ?? getNavLayout()
}

export function getServerNavLayoutSnapshot(): NavLayout {
  return 'desktop'
}
