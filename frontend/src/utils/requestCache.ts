/**
 * API 请求缓存管理器
 * 使用内存缓存和 TTL 机制减少重复请求
 */

interface CacheEntry<T> {
  data: T
  timestamp: number
  ttl: number // Time to live in milliseconds
}

class RequestCache {
  private cache: Map<string, CacheEntry<any>>
  private pendingRequests: Map<string, Promise<any>>

  constructor() {
    this.cache = new Map()
    this.pendingRequests = new Map()
  }

  /**
   * 获取缓存数据
   * @param key 缓存键
   * @returns 缓存的数据或 null
   */
  get<T>(key: string): T | null {
    const entry = this.cache.get(key)

    if (!entry) {
      return null
    }

    // 检查是否过期
    const now = Date.now()
    if (now - entry.timestamp > entry.ttl) {
      this.cache.delete(key)
      return null
    }

    return entry.data as T
  }

  /**
   * 设置缓存数据
   * @param key 缓存键
   * @param data 要缓存的数据
   * @param ttl 存活时间（毫秒），默认 5 分钟
   */
  set<T>(key: string, data: T, ttl: number = 5 * 60 * 1000): void {
    this.cache.set(key, {
      data,
      timestamp: Date.now(),
      ttl,
    })
  }

  /**
   * 删除指定缓存
   * @param key 缓存键
   */
  delete(key: string): void {
    this.cache.delete(key)
    this.pendingRequests.delete(key)
  }

  /**
   * 清空所有缓存
   */
  clear(): void {
    this.cache.clear()
    this.pendingRequests.clear()
  }

  /**
   * 包装请求，自动处理缓存和请求去重
   * @param key 缓存键
   * @param fetcher 获取数据的函数
   * @param ttl 缓存存活时间
   * @returns 数据 Promise
   */
  async fetch<T>(
    key: string,
    fetcher: () => Promise<T>,
    ttl?: number,
  ): Promise<T> {
    // 1. 检查缓存
    const cached = this.get<T>(key)
    if (cached !== null) {
      return cached
    }

    // 2. 检查是否有相同请求正在进行（请求去重）
    const pending = this.pendingRequests.get(key)
    if (pending) {
      return pending as Promise<T>
    }

    // 3. 发起新请求
    const promise = fetcher()
      .then((data) => {
        this.set(key, data, ttl)
        this.pendingRequests.delete(key)
        return data
      })
      .catch((error) => {
        this.pendingRequests.delete(key)
        throw error
      })

    this.pendingRequests.set(key, promise)
    return promise
  }

  /**
   * 获取缓存大小
   */
  get size(): number {
    return this.cache.size
  }

  /**
   * 获取所有缓存键
   */
  get keys(): string[] {
    return Array.from(this.cache.keys())
  }
}

// 导出单例实例
export const requestCache = new RequestCache()

// 为方便使用，导出包装好的 fetch 函数
export async function cachedFetch<T>(
  key: string,
  url: string,
  options?: RequestInit,
  ttl?: number,
): Promise<T> {
  return requestCache.fetch(
    key,
    async () => {
      const response = await fetch(url, options)
      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`)
      }
      return response.json()
    },
    ttl,
  )
}
