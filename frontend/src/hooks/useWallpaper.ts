/**
 * 壁纸管理 Hook
 *
 * 提供壁纸加载、刷新和状态管理功能
 * 确保壁纸URL和颜色提取的一致性
 *
 * @module useWallpaper
 * @version 2.0
 */

import { useCallback, useEffect, useState } from 'react'
import { API_URL } from '../config'
import { fetchJsonWithRetry } from '../utils/apiRetry'
import { loadImagePooled } from '../utils/objectPool'
import { getCacheInfo } from '../utils/wallpaperColorCache'
import {
  areUrlsEquivalent,
  extractBackgroundUrl,
  // 向后兼容导出
  getActiveWallpaperUrl,
  getDOMWallpaperUrl,
  getWallpaperApplyTimestamp,
  isWallpaperUrlActive,
  normalizeWallpaperUrl,
  wallpaperState,
} from '../utils/wallpaperState'

// 重新导出向后兼容函数
export {
  areUrlsEquivalent,
  getActiveWallpaperUrl,
  getDOMWallpaperUrl,
  getWallpaperApplyTimestamp,
  isWallpaperUrlActive,
  normalizeWallpaperUrl,
}

// ============================================================================
// 类型定义
// ============================================================================

interface WallpaperConfig {
  wallpaper_url: string
  wallpaper_blur: number
  // Evocative 壁纸动效配置
  evocative_parallax?: boolean
  evocative_dynamic_blur?: boolean
  evocative_ripple?: boolean
  evocative_fps?: number
  evocative_ripple_quality?: number
}

interface LoadWallpaperResult {
  /** 验证后的实际URL */
  actualUrl: string
  /** 模糊度 */
  blur: number
  /** URL是否经过验证 */
  verified: boolean
  /** @deprecated 使用 evocative 替代 */
  parallaxEnabled: boolean
  /** Evocative 壁纸动效配置 */
  evocative: {
    parallax: boolean
    dynamicBlur: boolean
    ripple: boolean
    fps: number
    rippleQuality: number
  }
}

// ============================================================================
// 常量
// ============================================================================

/** 图片加载超时时间 */
const IMAGE_LOAD_TIMEOUT = 15000

/** 壁纸元素ID */
const WALLPAPER_ELEMENT_ID = 'wallpaper'

/** 随机图片服务列表 */
const RANDOM_IMAGE_SERVICES = [
  'picsum.photos',
  'loremflickr.com',
  'source.unsplash.com',
  'unsplash.com/random',
  'api.unsplash.com',
  'bing.com/hpimagearchive',
] as const

/** 静态CDN标识 */
const STATIC_CDN_INDICATORS = [
  'cdn.',
  'static.',
  '/static/',
  '/images/',
  '/assets/',
  '/uploads/',
] as const

/** 图片扩展名 */
const IMAGE_EXTENSIONS = ['.jpg', '.jpeg', '.png', '.gif', '.webp', '.bmp', '.svg'] as const

/** 动态脚本扩展名 */
const DYNAMIC_EXTENSIONS = ['.php', '.jsp', '.asp', '.aspx', '.py'] as const

// ============================================================================
// 工具函数
// ============================================================================

/**
 * 从颜色缓存中获取一个不同于当前URL的已缓存壁纸
 * 利用 wallpaperColorCache 的缓存信息，避免重复维护缓存
 * @param currentUrl 当前壁纸URL
 * @returns 缓存中的其他壁纸URL，如果没有则返回null
 */
function getCachedAlternativeWallpaper(currentUrl: string | null): string | null {
  try {
    const cacheInfo = getCacheInfo()
    if (!cacheInfo.exists || !cacheInfo.items || cacheInfo.items.length < 2) {
      return null
    }

    // 从颜色缓存中提取完整URL（getCacheInfo返回的是截断的URL用于调试）
    // 需要直接读取localStorage获取完整URL
    const cached = localStorage.getItem('myriad_wallpaper_color_cache_v5')
    if (!cached)
      return null

    const store = JSON.parse(cached)
    if (!store.items || store.items.length < 2)
      return null

    // 过滤掉当前URL和过期项
    const now = Date.now()
    const CACHE_DURATION_MS = 6 * 60 * 60 * 1000 // 6小时
    const alternatives = store.items.filter((item: { url: string, timestamp: number }) => {
      // 过滤过期项
      if (now - item.timestamp > CACHE_DURATION_MS)
        return false
      // 过滤当前URL
      if (areUrlsEquivalent(item.url, currentUrl))
        return false
      return true
    })

    if (alternatives.length === 0)
      return null

    // 随机选择一个
    const randomIndex = Math.floor(Math.random() * alternatives.length)
    return alternatives[randomIndex].url
  }
  catch {
    return null
  }
}

/**
 * 判断URL是否为单一静态图片链接
 * 返回true表示是固定的静态图片，不应显示刷新按钮
 */
function isStaticImageUrl(url: string): boolean {
  if (!url)
    return true

  const lowerUrl = url.toLowerCase()

  // 随机图片服务 → 可刷新
  if (RANDOM_IMAGE_SERVICES.some(service => lowerUrl.includes(service))) {
    return false
  }

  // 包含 /random 或 /daily 路径 → 可刷新
  if (lowerUrl.includes('/random') || lowerUrl.includes('/daily')) {
    return false
  }

  // 动态脚本 → 可刷新
  if (DYNAMIC_EXTENSIONS.some(ext => lowerUrl.endsWith(ext))) {
    return false
  }

  // 不以图片扩展名结尾 → 可能是API → 可刷新
  const endsWithImage = IMAGE_EXTENSIONS.some(ext => lowerUrl.endsWith(ext))
  if (!endsWithImage) {
    return false
  }

  // 静态CDN图片 → 不可刷新
  if (STATIC_CDN_INDICATORS.some(indicator => lowerUrl.includes(indicator))) {
    return true
  }

  // 默认可刷新（保守策略）
  return false
}

/**
 * 获取实际的图片URL（处理重定向）
 */
async function resolveImageUrl(apiUrl: string, bustCache = false): Promise<string> {
  const url = bustCache
    ? apiUrl.includes('?')
      ? `${apiUrl}&t=${Date.now()}`
      : `${apiUrl}?t=${Date.now()}`
    : apiUrl

  try {
    const response = await fetch(url, { method: 'HEAD' })
    return response.url
  }
  catch {
    // 如果HEAD请求失败，返回原始URL
    return url
  }
}

/**
 * 预加载图片（使用对象池）
 * 直接加载图片，不使用代理
 * @returns 加载成功返回true，失败返回false
 */
function preloadImage(url: string, timeout = IMAGE_LOAD_TIMEOUT): Promise<boolean> {
  // 使用池化的图片加载，减少 GC 压力
  return loadImagePooled(url, { timeout })
}

/**
 * 应用壁纸到DOM并更新全局状态
 * @param imageUrl 目标图片URL
 * @param blur 模糊度
 * @param forceRefresh 是否强制刷新（即使URL相同）
 * @returns 验证后的URL，失败返回null
 */
async function applyWallpaperToDOM(
  imageUrl: string,
  blur: number,
  forceRefresh = false,
): Promise<string | null> {
  const wallpaperEl = document.getElementById(WALLPAPER_ELEMENT_ID)
  if (!wallpaperEl) {
    console.warn('壁纸元素不存在')
    return null
  }

  // 🔒 检查是否需要更新：如果当前壁纸与目标相同且不是强制刷新，跳过
  const currentUrl = extractBackgroundUrl(WALLPAPER_ELEMENT_ID)
  if (!forceRefresh && currentUrl && areUrlsEquivalent(currentUrl, imageUrl)) {
    // 已经是目标壁纸，只需更新模糊度（如果不同）
    const currentFilter = wallpaperEl.style.filter
    const targetFilter = `blur(${blur}px)`
    if (currentFilter !== targetFilter) {
      wallpaperEl.style.filter = targetFilter
    }
    // 确保状态同步
    wallpaperState.updateState(imageUrl, blur)
    return imageUrl
  }

  // 标记加载状态
  wallpaperState.setLoading(true)

  try {
    // 预加载图片
    const loaded = await preloadImage(imageUrl)
    if (!loaded) {
      wallpaperState.setError('图片加载失败')
      return null
    }

    // 🔒 再次检查：预加载期间可能已经切换到目标壁纸
    const currentUrlAfterLoad = extractBackgroundUrl(WALLPAPER_ELEMENT_ID)
    if (!forceRefresh && currentUrlAfterLoad && areUrlsEquivalent(currentUrlAfterLoad, imageUrl)) {
      wallpaperState.updateState(imageUrl, blur)
      return imageUrl
    }

    // 应用到DOM（使用渐变过渡减少闪烁）
    wallpaperEl.style.backgroundImage = `url(${imageUrl})`
    wallpaperEl.style.filter = `blur(${blur}px)`

    // 更新全局状态
    wallpaperState.updateState(imageUrl, blur)

    // 等待下一帧验证DOM更新
    await new Promise(resolve => requestAnimationFrame(resolve))

    // 验证DOM是否已更新
    const appliedUrl = extractBackgroundUrl(WALLPAPER_ELEMENT_ID)

    if (appliedUrl && areUrlsEquivalent(appliedUrl, imageUrl)) {
      return imageUrl
    }

    // 二次验证
    await new Promise(resolve => setTimeout(resolve, 50))
    const retryUrl = extractBackgroundUrl(WALLPAPER_ELEMENT_ID)

    if (retryUrl && areUrlsEquivalent(retryUrl, imageUrl)) {
      return imageUrl
    }

    console.warn('壁纸应用验证失败', { expected: imageUrl, actual: retryUrl })
    return retryUrl || imageUrl
  }
  catch (error) {
    const message = error instanceof Error ? error.message : '未知错误'
    wallpaperState.setError(message)
    return null
  }
  finally {
    wallpaperState.setLoading(false)
  }
}

/**
 * 获取壁纸配置（带自动重试）
 */
async function fetchWallpaperConfig(): Promise<WallpaperConfig | null> {
  console.debug('[Wallpaper] Fetching wallpaper config...')
  try {
    const data = await fetchJsonWithRetry<any>(`${API_URL}/api/config/ui`, {
      maxRetries: 3,
      timeout: 10000,
      onRetry: (error, attempt, delay) => {
        console.warn(
          `壁纸配置获取失败 (尝试 ${attempt}): ${error.message}. ${delay}ms后重试...`,
        )
      },
    })

    console.debug('[Wallpaper] Config received:', {
      wallpaper_url: data.wallpaper_url,
      blur: data.wallpaper_blur,
      evocative: {
        parallax: data.evocative_parallax,
        dynamicBlur: data.evocative_dynamic_blur,
        ripple: data.evocative_ripple,
        fps: data.evocative_fps,
        rippleQuality: data.evocative_ripple_quality,
      },
    })

    if (data.wallpaper_url) {
      return {
        wallpaper_url: data.wallpaper_url,
        wallpaper_blur: data.wallpaper_blur ?? 3,
        // Evocative 壁纸动效配置
        evocative_parallax: data.evocative_parallax ?? true,
        evocative_dynamic_blur: data.evocative_dynamic_blur ?? false,
        evocative_ripple: data.evocative_ripple ?? false,
        evocative_fps: data.evocative_fps ?? 30,
        evocative_ripple_quality: data.evocative_ripple_quality ?? 0.85,
      }
    }
    console.debug('[Wallpaper] No wallpaper_url in config')
    return null
  }
  catch (error) {
    console.error('壁纸配置获取失败:', error)
    return null
  }
}

// ============================================================================
// Hook 实现
// ============================================================================

/**
 * loadWallpaper 去重机制
 * 多个组件同时调用 loadWallpaper 时，只执行一次实际加载
 */
let pendingLoadWallpaper: Promise<LoadWallpaperResult | null> | null = null
let lastLoadTimestamp = 0
const LOAD_DEBOUNCE_MS = 1000 // 1秒内的重复调用直接返回上次结果
let lastLoadResult: LoadWallpaperResult | null = null

/**
 * 壁纸管理 Hook
 */
export function useWallpaper() {
  const [wallpaperUrl, setWallpaperUrl] = useState<string>('')
  const [canRefresh, setCanRefresh] = useState<boolean>(false)
  const [blur, setBlur] = useState<number>(3)
  const [isLoading, setIsLoading] = useState<boolean>(false)

  // 订阅全局状态变化
  useEffect(() => {
    return wallpaperState.subscribe((snapshot) => {
      setIsLoading(snapshot.isLoading)
    })
  }, [])

  /**
   * 加载壁纸配置和显示（带去重）
   * 多个组件同时调用时，只执行一次实际加载
   */
  const loadWallpaper = useCallback(async (): Promise<LoadWallpaperResult | null> => {
    const now = Date.now()
    console.debug('[Wallpaper] loadWallpaper called')

    // 1秒内的重复调用，直接返回上次结果
    if (lastLoadResult && now - lastLoadTimestamp < LOAD_DEBOUNCE_MS) {
      console.debug('[Wallpaper] Returning cached result (debounce)')
      // 同步本地状态
      if (lastLoadResult.actualUrl) {
        setWallpaperUrl(lastLoadResult.actualUrl)
        setBlur(lastLoadResult.blur)
      }
      return lastLoadResult
    }

    // 如果有正在进行的加载，等待其完成
    if (pendingLoadWallpaper) {
      console.debug('[Wallpaper] Waiting for pending load...')
      const result = await pendingLoadWallpaper
      // 同步本地状态
      if (result?.actualUrl) {
        setWallpaperUrl(result.actualUrl)
        setBlur(result.blur)
      }
      return result
    }

    console.debug('[Wallpaper] Starting new load...')
    // 执行实际加载
    const doLoad = async (): Promise<LoadWallpaperResult | null> => {
      try {
        const config = await fetchWallpaperConfig()
        if (!config) {
          return null
        }

        const actualUrl = await resolveImageUrl(config.wallpaper_url)

        // 验证URL有效性
        if (!actualUrl || actualUrl.includes('/api/proxy/music/')) {
          return null
        }

        // 应用到DOM并验证
        const verifiedUrl = await applyWallpaperToDOM(actualUrl, config.wallpaper_blur)

        if (!verifiedUrl) {
          return null
        }

        const result: LoadWallpaperResult = {
          actualUrl: verifiedUrl,
          blur: config.wallpaper_blur,
          verified: areUrlsEquivalent(verifiedUrl, actualUrl),
          // 兼容旧配置
          parallaxEnabled: config.evocative_parallax ?? true,
          // 新的 Evocative 配置
          evocative: {
            parallax: config.evocative_parallax ?? true,
            dynamicBlur: config.evocative_dynamic_blur ?? false,
            ripple: config.evocative_ripple ?? false,
            fps: config.evocative_fps ?? 30,
            rippleQuality: config.evocative_ripple_quality ?? 0.85,
          },
        }

        // 缓存结果
        lastLoadResult = result
        lastLoadTimestamp = Date.now()

        // 更新本地状态
        setWallpaperUrl(verifiedUrl)
        setBlur(config.wallpaper_blur)
        setCanRefresh(!isStaticImageUrl(config.wallpaper_url))

        return result
      }
      catch (error) {
        console.error('加载壁纸失败:', error)
        return null
      }
    }

    // 设置 pending Promise
    pendingLoadWallpaper = doLoad()

    try {
      const result = await pendingLoadWallpaper
      return result
    }
    finally {
      // 清除 pending（延迟清除，避免并发问题）
      setTimeout(() => {
        pendingLoadWallpaper = null
      }, 100)
    }
  }, [])

  /**
   * 刷新壁纸
   * 优先从颜色缓存中选择不同于当前的壁纸，如果缓存不足则请求新图片
   */
  const refreshWallpaper = useCallback(async (): Promise<string | null> => {
    try {
      const config = await fetchWallpaperConfig()
      if (!config) {
        return null
      }

      // 获取当前壁纸URL
      const currentUrl = wallpaperUrl || extractBackgroundUrl(WALLPAPER_ELEMENT_ID)

      // 优先尝试从颜色缓存获取不同的壁纸（复用已缓存的颜色信息）
      const cachedAlternative = getCachedAlternativeWallpaper(currentUrl)

      let targetUrl: string

      if (cachedAlternative) {
        // 使用缓存中的壁纸（已有颜色缓存，切换更快）
        targetUrl = cachedAlternative
      }
      else {
        // 缓存不足，请求新图片（添加时间戳避免缓存）
        targetUrl = await resolveImageUrl(config.wallpaper_url, true)

        // 验证URL有效性
        if (!targetUrl || targetUrl.includes('/api/proxy/music/')) {
          return null
        }

        // 检查新URL是否与当前相同
        if (areUrlsEquivalent(targetUrl, currentUrl)) {
          // 如果API返回了相同的URL，再尝试一次
          await new Promise(resolve => setTimeout(resolve, 100))
          targetUrl = await resolveImageUrl(config.wallpaper_url, true)

          // 如果还是相同，直接返回
          if (areUrlsEquivalent(targetUrl, currentUrl)) {
            return currentUrl
          }
        }
      }

      // 应用到DOM并验证（强制刷新）
      const verifiedUrl = await applyWallpaperToDOM(targetUrl, config.wallpaper_blur, true)

      if (!verifiedUrl) {
        return null
      }

      // 更新本地状态
      setWallpaperUrl(verifiedUrl)
      setBlur(config.wallpaper_blur)

      // 触发事件通知其他组件
      window.dispatchEvent(
        new CustomEvent('wallpaperChanged', {
          detail: {
            url: verifiedUrl,
            timestamp: wallpaperState.getAppliedTimestamp(),
            fromCache: !!cachedAlternative,
          },
        }),
      )

      return verifiedUrl
    }
    catch (error) {
      console.error('刷新壁纸失败:', error)
      return null
    }
  }, [wallpaperUrl])

  return {
    wallpaperUrl,
    canRefresh,
    blur,
    isLoading,
    loadWallpaper,
    refreshWallpaper,
  }
}
