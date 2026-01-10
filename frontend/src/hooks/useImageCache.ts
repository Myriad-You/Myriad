/**
 * 图片请求缓存 Hook
 *
 * 用于防止图片重复请求导致 429 错误
 * - 在内存中缓存已请求的图片 URL
 * - 防止组件重渲染时重复触发图片请求
 * - 自动清理过期缓存
 */

// 图片缓存状态
interface ImageCacheEntry {
  status: 'loading' | 'loaded' | 'error'
  timestamp: number
}

// 全局图片缓存 Map
const imageCache = new Map<string, ImageCacheEntry>()

// 缓存过期时间（5分钟）
const CACHE_EXPIRY = 5 * 60 * 1000

// 最大缓存数量
const MAX_CACHE_SIZE = 500

// 请求去重 Map（防止并发重复请求）
const pendingRequests = new Map<string, Promise<boolean>>()

/**
 * 清理过期缓存
 */
function cleanupExpiredCache(): void {
  const now = Date.now()
  for (const [url, entry] of imageCache.entries()) {
    if (now - entry.timestamp > CACHE_EXPIRY) {
      imageCache.delete(url)
    }
  }

  // 如果缓存仍然过大，删除最旧的条目
  if (imageCache.size > MAX_CACHE_SIZE) {
    const entries = Array.from(imageCache.entries())
      .sort((a, b) => a[1].timestamp - b[1].timestamp)
    const toDelete = entries.slice(0, entries.length - MAX_CACHE_SIZE)
    for (const [url] of toDelete) {
      imageCache.delete(url)
    }
  }
}

/**
 * 预加载图片
 */
export function preloadImage(url: string): Promise<boolean> {
  // 空 URL 直接返回
  if (!url)
    return Promise.resolve(false)

  // 检查缓存
  const cached = imageCache.get(url)
  if (cached && Date.now() - cached.timestamp < CACHE_EXPIRY) {
    return Promise.resolve(cached.status === 'loaded')
  }

  // 检查是否有正在进行的请求
  const pending = pendingRequests.get(url)
  if (pending) {
    return pending
  }

  // 创建新的加载请求
  const loadPromise = new Promise<boolean>((resolve) => {
    const img = new Image()

    img.onload = () => {
      imageCache.set(url, { status: 'loaded', timestamp: Date.now() })
      pendingRequests.delete(url)
      resolve(true)
    }

    img.onerror = () => {
      imageCache.set(url, { status: 'error', timestamp: Date.now() })
      pendingRequests.delete(url)
      resolve(false)
    }

    img.src = url
  })

  // 标记为加载中
  imageCache.set(url, { status: 'loading', timestamp: Date.now() })
  pendingRequests.set(url, loadPromise)

  return loadPromise
}

/**
 * 检查图片是否已缓存
 */
export function isImageCached(url: string): boolean {
  if (!url)
    return false
  const cached = imageCache.get(url)
  return cached?.status === 'loaded' && Date.now() - cached.timestamp < CACHE_EXPIRY
}

/**
 * 检查图片是否加载失败
 */
export function isImageError(url: string): boolean {
  if (!url)
    return false
  const cached = imageCache.get(url)
  return cached?.status === 'error' && Date.now() - cached.timestamp < CACHE_EXPIRY
}

/**
 * 获取缓存的图片 URL（如果已缓存则返回，否则返回 null）
 */
export function getCachedImageUrl(url: string | null | undefined): string | null {
  if (!url)
    return null

  // 清理过期缓存
  if (imageCache.size > MAX_CACHE_SIZE / 2) {
    cleanupExpiredCache()
  }

  // 如果已缓存且成功，返回 URL
  if (isImageCached(url)) {
    return url
  }

  // 如果已缓存但失败，返回 null
  if (isImageError(url)) {
    return null
  }

  // 触发预加载（异步，不阻塞）
  preloadImage(url)

  return url
}

/**
 * 手动清除缓存
 */
export function clearImageCache(): void {
  imageCache.clear()
  pendingRequests.clear()
}

// 定期清理缓存（每分钟）
if (typeof window !== 'undefined') {
  setInterval(cleanupExpiredCache, 60 * 1000)
}
