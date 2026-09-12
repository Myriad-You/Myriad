export interface WallpaperState {
  activeUrl: string | null
  originalUrl: string | null
  appliedAt: number
  blur: number
  isLoading: boolean
  lastError: string | null
}

export interface WallpaperStateSnapshot extends Readonly<WallpaperState> {
  snapshotAt: number
}

type WallpaperStateListener = (state: WallpaperStateSnapshot) => void

const CACHE_BUST_PARAMS = [
  't',
  '_t',
  'timestamp',
  'cache',
  'v',
  'cachebust',
  'nocache',
] as const

const WALLPAPER_ELEMENT_ID = 'wallpaper'

export function normalizeWallpaperUrl(url: string): string {
  if (!url) return ''

  try {
    const urlObj = new URL(url)
    CACHE_BUST_PARAMS.forEach((param) => urlObj.searchParams.delete(param))
    return urlObj.toString()
  } catch {
    return url
  }
}

export function areUrlsEquivalent(
  url1: string | null,
  url2: string | null,
): boolean {
  if (!url1 || !url2) return false
  return normalizeWallpaperUrl(url1) === normalizeWallpaperUrl(url2)
}

export function extractBackgroundUrl(
  elementId: string = WALLPAPER_ELEMENT_ID,
): string | null {
  const element = document.getElementById(elementId)
  if (!element) return null

  const bgImage = element.style.backgroundImage
  if (!bgImage || bgImage === 'none') return null

  const match = bgImage.match(/url\(["']?([^"')]+)["']?\)/)
  return match?.[1] ?? null
}

class WallpaperStateManager {
  private state: WallpaperState = {
    activeUrl: null,
    originalUrl: null,
    appliedAt: 0,
    blur: 3,
    isLoading: false,
    lastError: null,
  }

  private listeners: Set<WallpaperStateListener> = new Set()
  private pendingNotification: number | null = null

  getSnapshot(): WallpaperStateSnapshot {
    return {
      ...this.state,
      snapshotAt: Date.now(),
    }
  }

  getActiveUrl(): string | null {
    return this.state.activeUrl
  }

  getAppliedTimestamp(): number {
    return this.state.appliedAt
  }

  isUrlActive(url: string): boolean {
    return areUrlsEquivalent(url, this.state.activeUrl)
  }

  isDOMConsistent(): boolean {
    const domUrl = extractBackgroundUrl()
    return areUrlsEquivalent(domUrl, this.state.activeUrl)
  }

  setLoading(isLoading: boolean): void {
    this.state.isLoading = isLoading
    this.notifyListeners()
  }

  setError(error: string | null): void {
    this.state.lastError = error
    this.notifyListeners()
  }

  updateState(url: string, blur: number = 3): void {
    const normalizedUrl = normalizeWallpaperUrl(url)

    this.state = {
      activeUrl: normalizedUrl,
      originalUrl: url,
      appliedAt: Date.now(),
      blur,
      isLoading: false,
      lastError: null,
    }

    this.notifyListeners()
  }

  clearState(): void {
    this.state = {
      activeUrl: null,
      originalUrl: null,
      appliedAt: 0,
      blur: 3,
      isLoading: false,
      lastError: null,
    }
    this.notifyListeners()
  }

  subscribe(listener: WallpaperStateListener): () => void {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  private notifyListeners(): void {
    // Debounce on microtask.
    if (this.pendingNotification !== null) {
      cancelAnimationFrame(this.pendingNotification)
    }

    this.pendingNotification = requestAnimationFrame(() => {
      this.pendingNotification = null
      const snapshot = this.getSnapshot()
      this.listeners.forEach((listener) => {
        try {
          listener(snapshot)
        } catch (error) {
          console.error('壁纸状态监听器执行错误:', error)
        }
      })
    })
  }

  getDebugInfo(): {
    state: WallpaperStateSnapshot
    domUrl: string | null
    isConsistent: boolean
    listenerCount: number
  } {
    const domUrl = extractBackgroundUrl()
    return {
      state: this.getSnapshot(),
      domUrl,
      isConsistent: this.isDOMConsistent(),
      listenerCount: this.listeners.size,
    }
  }
}

export const wallpaperState = new WallpaperStateManager()

/** Liquid glass needs a sharp wallpaper; do not pre-blur it. */
const LIQUID_WALLPAPER_BLUR_SCALE = 0.35

export function effectiveWallpaperBlur(blur: number): number {
  if (
    typeof document !== 'undefined'
    && document.documentElement.dataset.surface === 'liquid'
  ) {
    return Math.round(blur * LIQUID_WALLPAPER_BLUR_SCALE * 10) / 10
  }
  return blur
}

export function resyncWallpaperBlur(): void {
  if (typeof document === 'undefined') return
  const el = document.getElementById(WALLPAPER_ELEMENT_ID)
  if (!el || !wallpaperState.getActiveUrl()) return
  const blur = effectiveWallpaperBlur(wallpaperState.getSnapshot().blur)
  el.style.filter = `blur(${blur}px)`
}
