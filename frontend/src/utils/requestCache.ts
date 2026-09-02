/**
 * API 请求缓存管理器
 * 使用内存缓存 + TTL + LRU 上限，减少重复请求并防止缓存无限增长
 */

interface CacheEntry<T> {
  data: T
  timestamp: number
  ttl: number // Time to live in milliseconds
}

/** 默认最大缓存条目数（超出后按 LRU 淘汰最久未访问项） */
const DEFAULT_MAX_ENTRIES = 80

/** 过期扫描间隔（ms） */
const SWEEP_INTERVAL_MS = 60_000

class RequestCache {
  private cache: Map<string, CacheEntry<any>>
  private pendingRequests: Map<string, Promise<any>>
  private maxEntries: number
  private sweepTimer: ReturnType<typeof setTimeout> | null = null

  constructor(maxEntries: number = DEFAULT_MAX_ENTRIES) {
    this.cache = new Map()
    this.pendingRequests = new Map()
    this.maxEntries = maxEntries
  }

  /**
   * 缓存非空时才安排下一次过期扫描。
   * 空缓存不保留常驻 interval，避免应用空闲时每分钟无意义唤醒。
   */
  private scheduleSweep(): void {
    if (typeof window === 'undefined') return
    if (this.sweepTimer || this.cache.size === 0) return

    this.sweepTimer = setTimeout(() => {
      this.sweepTimer = null
      this.sweepExpired()
      this.scheduleSweep()
    }, SWEEP_INTERVAL_MS)

    // Node / 测试环境可能无 unref；浏览器忽略
    if (
      typeof this.sweepTimer === 'object' &&
      this.sweepTimer !== null &&
      'unref' in this.sweepTimer
    ) {
      ;(this.sweepTimer as NodeJS.Timeout).unref?.()
    }
  }

  private stopSweepIfIdle(): void {
    if (this.cache.size !== 0 || this.sweepTimer === null) return
    clearTimeout(this.sweepTimer)
    this.sweepTimer = null
  }

  /**
   * 删除所有已过期条目
   */
  sweepExpired(): number {
    const now = Date.now()
    let removed = 0
    for (const [key, entry] of this.cache) {
      if (now - entry.timestamp > entry.ttl) {
        this.cache.delete(key)
        removed++
      }
    }
    this.stopSweepIfIdle()
    return removed
  }

  /**
   * 写入后维持 LRU 上限：Map 保持插入顺序，队头为最旧
   */
  private enforceLimit(): void {
    while (this.cache.size > this.maxEntries) {
      const oldest = this.cache.keys().next().value
      if (oldest === undefined) break
      this.cache.delete(oldest)
    }
  }

  /**
   * 获取缓存数据（命中时刷新 LRU 顺序）
   */
  get<T>(key: string): T | null {
    const entry = this.cache.get(key)

    if (!entry) {
      return null
    }

    const now = Date.now()
    if (now - entry.timestamp > entry.ttl) {
      this.cache.delete(key)
      return null
    }

    // 重新插入到末尾 → 标记为最近使用
    this.cache.delete(key)
    this.cache.set(key, entry)

    return entry.data as T
  }

  /**
   * 设置缓存数据
   * @param ttl 存活时间（毫秒），默认 5 分钟
   */
  set<T>(key: string, data: T, ttl: number = 5 * 60 * 1000): void {
    // 先删再写，确保 key 位于 LRU 末尾
    this.cache.delete(key)
    this.cache.set(key, {
      data,
      timestamp: Date.now(),
      ttl,
    })
    this.enforceLimit()
    this.scheduleSweep()
  }

  /**
   * 删除指定缓存
   */
  delete(key: string): void {
    this.cache.delete(key)
    this.pendingRequests.delete(key)
    this.stopSweepIfIdle()
  }

  /**
   * 按前缀批量删除（如离开 Brew 时清理 brew:）
   */
  deleteByPrefix(prefix: string): number {
    let removed = 0
    for (const key of [...this.cache.keys()]) {
      if (key.startsWith(prefix)) {
        this.cache.delete(key)
        removed++
      }
    }
    for (const key of [...this.pendingRequests.keys()]) {
      if (key.startsWith(prefix)) {
        this.pendingRequests.delete(key)
      }
    }
    this.stopSweepIfIdle()
    return removed
  }

  /**
   * 清空所有缓存
   */
  clear(): void {
    this.cache.clear()
    this.pendingRequests.clear()
    this.stopSweepIfIdle()
  }

  /**
   * 包装请求，自动处理缓存和请求去重
   */
  async fetch<T>(
    key: string,
    fetcher: () => Promise<T>,
    ttl?: number,
  ): Promise<T> {
    const cached = this.get<T>(key)
    if (cached !== null) {
      return cached
    }

    const pending = this.pendingRequests.get(key)
    if (pending) {
      return pending as Promise<T>
    }

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

  get size(): number {
    return this.cache.size
  }

  get keys(): string[] {
    return Array.from(this.cache.keys())
  }

  /** 调试状态 */
  getStatus() {
    return {
      size: this.cache.size,
      maxEntries: this.maxEntries,
      pending: this.pendingRequests.size,
    }
  }
}

// 导出单例实例
export const requestCache = new RequestCache()
