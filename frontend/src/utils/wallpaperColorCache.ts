/**
 * 壁纸颜色缓存管理工具
 *
 * 功能：
 * - 6小时缓存有效期
 * - 基于标准化URL的LRU缓存
 * - 自动清理过期缓存
 * - 与当前活跃壁纸的一致性验证
 *
 * @module wallpaperColorCache
 * @version 2.0
 */

import type { ColorPalette } from './colorExtractor'
import {
  areUrlsEquivalent,
  extractBackgroundUrl,
  normalizeWallpaperUrl,
  wallpaperState,
} from './wallpaperState'

// ============================================================================
// 类型定义
// ============================================================================

interface WallpaperColorCacheItem {
  /** 标准化后的URL */
  url: string
  /** 颜色配色 */
  palette: ColorPalette
  /** 缓存时间戳 */
  timestamp: number
  /** 访问次数（用于LRU） */
  accessCount: number
}

interface WallpaperColorCacheStore {
  version: number
  items: WallpaperColorCacheItem[]
}

interface ColorExtractionCheckResult {
  shouldApply: boolean
  cacheKey?: string
  reason?: string
}

// ============================================================================
// 常量
// ============================================================================

const CACHE_VERSION = 5 // 升级版本号
const CACHE_DURATION_MS = 6 * 60 * 60 * 1000 // 6小时
const CACHE_KEY = 'myriad_wallpaper_color_cache_v5'
const MAX_CACHE_ITEMS = 10
const MIN_IMAGE_SIZE = 100 // 最小图片尺寸（降低要求以支持更多图片源）

// ============================================================================
// 缓存存储操作
// ============================================================================

/**
 * 从localStorage读取缓存存储
 */
function getCacheStore(): WallpaperColorCacheStore | null {
  try {
    const cached = localStorage.getItem(CACHE_KEY)
    if (!cached)
      return null

    const store: WallpaperColorCacheStore = JSON.parse(cached)
    if (store.version !== CACHE_VERSION) {
      // 版本不匹配，清除旧缓存
      localStorage.removeItem(CACHE_KEY)
      return null
    }

    return store
  }
  catch {
    // 解析失败，清除损坏的缓存
    try {
      localStorage.removeItem(CACHE_KEY)
    }
    catch { /* 忽略 */ }
    return null
  }
}

/**
 * 保存缓存存储到localStorage
 */
function saveCacheStore(store: WallpaperColorCacheStore): boolean {
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify(store))
    return true
  }
  catch (error) {
    // localStorage可能已满或不可用
    console.warn('保存颜色缓存失败:', error)
    return false
  }
}

/**
 * 清理过期和超出限制的缓存项
 */
function cleanupCacheStore(store: WallpaperColorCacheStore): void {
  const now = Date.now()

  // 移除过期项
  store.items = store.items.filter(
    item => now - item.timestamp < CACHE_DURATION_MS,
  )

  // 如果仍超出限制，按访问次数和时间排序后裁剪
  if (store.items.length > MAX_CACHE_ITEMS) {
    store.items.sort((a, b) => {
      // 优先保留访问次数高的
      if (b.accessCount !== a.accessCount) {
        return b.accessCount - a.accessCount
      }
      // 次优先保留新的
      return b.timestamp - a.timestamp
    })
    store.items = store.items.slice(0, MAX_CACHE_ITEMS)
  }
}

// ============================================================================
// 公共API
// ============================================================================

/**
 * 检查URL是否适合进行颜色提取，并验证与当前壁纸的一致性
 * 简化验证流程，避免重复加载图片
 */
export async function shouldApplyColorExtraction(url: string): Promise<ColorExtractionCheckResult> {
  if (!url) {
    return { shouldApply: false, reason: 'URL为空' }
  }

  // 快速排除
  if (url.includes('/api/proxy/music/')) {
    return { shouldApply: false, reason: '音乐封面URL' }
  }
  if (url.startsWith('file://')) {
    return { shouldApply: false, reason: 'file://协议不支持' }
  }

  // 验证与当前活跃壁纸的一致性
  if (!wallpaperState.isUrlActive(url)) {
    const activeUrl = wallpaperState.getActiveUrl()
    console.debug('[ColorCache] URL mismatch:', {
      provided: url.substring(0, 60),
      active: activeUrl?.substring(0, 60),
    })
    return {
      shouldApply: false,
      reason: activeUrl
        ? `URL与当前壁纸不一致`
        : '没有活跃壁纸',
    }
  }

  // 验证DOM一致性（可选，跳过时更宽容）
  const domUrl = extractBackgroundUrl()
  if (domUrl && !areUrlsEquivalent(domUrl, url)) {
    // 仅记录警告，不阻止提取（DOM可能还未更新）
    console.debug('[ColorCache] DOM URL mismatch (may be timing issue):', {
      provided: url.substring(0, 60),
      dom: domUrl.substring(0, 60),
    })
  }

  // 不再预加载验证图片尺寸，由 extractFromImage 处理
  // 这避免了重复加载图片的性能问题
  return {
    shouldApply: true,
    cacheKey: normalizeWallpaperUrl(url),
  }
}

/**
 * 验证图片是否有效
 */
async function validateImage(url: string): Promise<{ valid: boolean, reason?: string }> {
  return new Promise((resolve) => {
    const img = new Image()
    img.crossOrigin = 'anonymous'

    const timeout = setTimeout(() => {
      img.src = ''
      resolve({ valid: false, reason: '图片加载超时' })
    }, 10000)

    img.onload = () => {
      clearTimeout(timeout)
      if (img.width < MIN_IMAGE_SIZE || img.height < MIN_IMAGE_SIZE) {
        resolve({ valid: false, reason: `图片太小: ${img.width}x${img.height}` })
      }
      else {
        resolve({ valid: true })
      }
    }

    img.onerror = () => {
      clearTimeout(timeout)
      resolve({ valid: false, reason: '图片加载失败' })
    }

    img.src = url
  })
}

/**
 * 从缓存获取颜色配色
 */
export function getColorFromCache(url: string): ColorPalette | null {
  try {
    const normalizedUrl = normalizeWallpaperUrl(url)
    const store = getCacheStore()
    if (!store)
      return null

    const item = store.items.find(i => i.url === normalizedUrl)
    if (!item)
      return null

    // 检查是否过期
    const age = Date.now() - item.timestamp
    if (age > CACHE_DURATION_MS) {
      // 异步清理过期项
      store.items = store.items.filter(i => i.url !== normalizedUrl)
      saveCacheStore(store)
      return null
    }

    // 更新访问次数
    item.accessCount++
    saveCacheStore(store)

    return item.palette
  }
  catch {
    return null
  }
}

/**
 * 保存颜色配色到缓存
 */
export function saveColorToCache(url: string, palette: ColorPalette): void {
  try {
    const normalizedUrl = normalizeWallpaperUrl(url)
    let store = getCacheStore()

    if (!store) {
      store = { version: CACHE_VERSION, items: [] }
    }

    // 移除已存在的相同URL项
    store.items = store.items.filter(item => item.url !== normalizedUrl)

    // 添加新项
    store.items.push({
      url: normalizedUrl,
      palette,
      timestamp: Date.now(),
      accessCount: 1,
    })

    // 清理
    cleanupCacheStore(store)
    saveCacheStore(store)
  }
  catch {
    // 静默失败
  }
}

/**
 * 清除所有壁纸颜色缓存
 */
export function clearColorCache(): void {
  try {
    localStorage.removeItem(CACHE_KEY)
  }
  catch {
    // 静默失败
  }
}

/**
 * 获取缓存调试信息
 */
export function getCacheInfo(): {
  exists: boolean
  count?: number
  totalSize?: number
  items?: Array<{ url: string, age: number, accessCount: number }>
} {
  try {
    const cached = localStorage.getItem(CACHE_KEY)
    if (!cached)
      return { exists: false }

    const store: WallpaperColorCacheStore = JSON.parse(cached)
    const now = Date.now()

    return {
      exists: true,
      count: store.items.length,
      totalSize: cached.length,
      items: store.items.map(item => ({
        url: item.url.length > 60 ? `${item.url.substring(0, 60)}...` : item.url,
        age: Math.round((now - item.timestamp) / 1000),
        accessCount: item.accessCount,
      })),
    }
  }
  catch {
    return { exists: false }
  }
}
