/**
 * 壁纸状态管理模块 (Singleton)
 *
 * 提供集中式的壁纸状态管理，确保壁纸URL和颜色提取的一致性
 *
 * @module wallpaperState
 */

// ============================================================================
// 类型定义
// ============================================================================

export interface WallpaperState {
  /** 当前活跃的壁纸URL（标准化后） */
  activeUrl: string | null
  /** 原始URL（未标准化） */
  originalUrl: string | null
  /** 壁纸应用时间戳 */
  appliedAt: number
  /** 壁纸模糊度 */
  blur: number
  /** 是否正在加载中 */
  isLoading: boolean
  /** 最后一次错误信息 */
  lastError: string | null
}

export interface WallpaperStateSnapshot extends Readonly<WallpaperState> {
  /** 获取状态的时间 */
  snapshotAt: number
}

type WallpaperStateListener = (state: WallpaperStateSnapshot) => void

// ============================================================================
// 常量
// ============================================================================

/** 用于标准化URL时需要移除的缓存参数 */
const CACHE_BUST_PARAMS = [
  't',
  '_t',
  'timestamp',
  'cache',
  'v',
  'cachebust',
  'nocache',
] as const

/** 壁纸元素ID */
const WALLPAPER_ELEMENT_ID = 'wallpaper'

// ============================================================================
// 工具函数
// ============================================================================

/**
 * 标准化URL：移除时间戳和缓存破坏参数
 * @param url 原始URL
 * @returns 标准化后的URL
 */
export function normalizeWallpaperUrl(url: string): string {
  if (!url) return ''

  try {
    const urlObj = new URL(url)
    CACHE_BUST_PARAMS.forEach((param) => urlObj.searchParams.delete(param))
    return urlObj.toString()
  } catch {
    // 如果URL无效，直接返回原始值
    return url
  }
}

/**
 * 比较两个URL是否指向同一资源（忽略缓存参数）
 * @param url1 第一个URL
 * @param url2 第二个URL
 * @returns 是否相同
 */
export function areUrlsEquivalent(
  url1: string | null,
  url2: string | null,
): boolean {
  if (!url1 || !url2) return false
  return normalizeWallpaperUrl(url1) === normalizeWallpaperUrl(url2)
}

/**
 * 从DOM元素提取背景图片URL
 * @param elementId 元素ID
 * @returns URL或null
 */
export function extractBackgroundUrl(
  elementId: string = WALLPAPER_ELEMENT_ID,
): string | null {
  const element = document.getElementById(elementId)
  if (!element) return null

  const bgImage = element.style.backgroundImage
  if (!bgImage || bgImage === 'none') return null

  // 匹配 url("...") 或 url('...') 或 url(...)
  const match = bgImage.match(/url\(["']?([^"')]+)["']?\)/)
  return match?.[1] ?? null
}

// ============================================================================
// 壁纸状态管理器 (Singleton)
// ============================================================================

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

  /**
   * 获取当前状态快照
   */
  getSnapshot(): WallpaperStateSnapshot {
    return {
      ...this.state,
      snapshotAt: Date.now(),
    }
  }

  /**
   * 获取当前活跃的壁纸URL
   */
  getActiveUrl(): string | null {
    return this.state.activeUrl
  }

  /**
   * 获取壁纸应用时间戳
   */
  getAppliedTimestamp(): number {
    return this.state.appliedAt
  }

  /**
   * 验证给定URL是否与当前活跃壁纸一致
   * @param url 要验证的URL
   * @returns 是否一致
   */
  isUrlActive(url: string): boolean {
    return areUrlsEquivalent(url, this.state.activeUrl)
  }

  /**
   * 验证DOM中显示的壁纸与状态是否一致
   * @returns 是否一致
   */
  isDOMConsistent(): boolean {
    const domUrl = extractBackgroundUrl()
    return areUrlsEquivalent(domUrl, this.state.activeUrl)
  }

  /**
   * 设置加载状态
   */
  setLoading(isLoading: boolean): void {
    this.state.isLoading = isLoading
    this.notifyListeners()
  }

  /**
   * 设置错误信息
   */
  setError(error: string | null): void {
    this.state.lastError = error
    this.notifyListeners()
  }

  /**
   * 更新壁纸状态
   * @param url 新的壁纸URL
   * @param blur 模糊度
   */
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

  /**
   * 清除壁纸状态
   */
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

  /**
   * 订阅状态变化
   * @param listener 监听器函数
   * @returns 取消订阅函数
   */
  subscribe(listener: WallpaperStateListener): () => void {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  /**
   * 通知所有监听器（防抖）
   */
  private notifyListeners(): void {
    // 使用微任务防抖，避免同步触发多次
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

  /**
   * 获取调试信息
   */
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

// ============================================================================
// 导出单例实例
// ============================================================================

/** 全局壁纸状态管理器实例 */
export const wallpaperState = new WallpaperStateManager()

// ============================================================================
// liquid 表面的壁纸模糊收敛
// ============================================================================

/**
 * liquid 表面下壁纸基础模糊的收敛系数。Liquid Glass 的光学前提是
 *  「壁纸保持清晰、玻璃负责模糊」：背后已经糊了，玻璃的折射与拾光
 *  就没有素材。其余表面维持配置原值。
 */
const LIQUID_WALLPAPER_BLUR_SCALE = 0.35

/**
 * 按当前表面主题折算壁纸模糊值 ——
 *  所有写 #wallpaper style.filter 的路径统一经过这里
 */
export function effectiveWallpaperBlur(blur: number): number {
  if (
    typeof document !== 'undefined'
    && document.documentElement.dataset.surface === 'liquid'
  ) {
    return Math.round(blur * LIQUID_WALLPAPER_BLUR_SCALE * 10) / 10
  }
  return blur
}

/**
 * 表面主题切换后立即重算壁纸模糊（useWidgetTheme.applyThemeToRoot 调用；
 *  evocative 动态模糊若在运行，会在后续帧以同一折算接管）
 */
export function resyncWallpaperBlur(): void {
  if (typeof document === 'undefined') return
  const el = document.getElementById(WALLPAPER_ELEMENT_ID)
  if (!el || !wallpaperState.getActiveUrl()) return
  const blur = effectiveWallpaperBlur(wallpaperState.getSnapshot().blur)
  el.style.filter = `blur(${blur}px)`
}

// ============================================================================
// 便捷导出函数（向后兼容）
// ============================================================================

/**
 * 获取当前活跃的壁纸URL
 * @deprecated 请使用 wallpaperState.getActiveUrl()
 */
export function getActiveWallpaperUrl(): string | null {
  return wallpaperState.getActiveUrl()
}

/**
 * 获取壁纸应用时间戳
 * @deprecated 请使用 wallpaperState.getAppliedTimestamp()
 */
export function getWallpaperApplyTimestamp(): number {
  return wallpaperState.getAppliedTimestamp()
}

/**
 * 验证给定URL是否与当前活跃壁纸一致
 * @deprecated 请使用 wallpaperState.isUrlActive()
 */
export function isWallpaperUrlActive(url: string): boolean {
  return wallpaperState.isUrlActive(url)
}

/**
 * 获取DOM中当前显示的壁纸URL
 * @deprecated 请使用 extractBackgroundUrl()
 */
export function getDOMWallpaperUrl(): string | null {
  return extractBackgroundUrl()
}
