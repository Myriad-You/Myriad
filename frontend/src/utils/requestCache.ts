interface CacheEntry<T> {
  data: T
  timestamp: number
  ttl: number
}

const DEFAULT_MAX_ENTRIES = 80

const SWEEP_INTERVAL_MS = 60_000

export class RequestCache {
  private cache: Map<string, CacheEntry<any>>
  private pendingRequests: Map<string, Promise<any>>
  private maxEntries: number
  private sweepTimer: ReturnType<typeof setTimeout> | null = null

  constructor(maxEntries: number = DEFAULT_MAX_ENTRIES) {
    this.cache = new Map()
    this.pendingRequests = new Map()
    this.maxEntries = maxEntries
  }

  private scheduleSweep(): void {
    if (typeof window === 'undefined') return
    if (this.sweepTimer || this.cache.size === 0) return

    this.sweepTimer = setTimeout(() => {
      this.sweepTimer = null
      this.sweepExpired()
      this.scheduleSweep()
    }, SWEEP_INTERVAL_MS)

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

  /** Map insertion order = LRU. */
  private enforceLimit(): void {
    while (this.cache.size > this.maxEntries) {
      const oldest = this.cache.keys().next().value
      if (oldest === undefined) break
      this.cache.delete(oldest)
    }
  }

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

    this.cache.delete(key)
    this.cache.set(key, entry)

    return entry.data as T
  }

  set<T>(key: string, data: T, ttl: number = 5 * 60 * 1000): void {
    this.pendingRequests.delete(key)
    this.cache.delete(key)
    this.cache.set(key, {
      data,
      timestamp: Date.now(),
      ttl,
    })
    this.enforceLimit()
    this.scheduleSweep()
  }

  delete(key: string): void {
    this.cache.delete(key)
    this.pendingRequests.delete(key)
    this.stopSweepIfIdle()
  }

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

  clear(): void {
    this.cache.clear()
    this.pendingRequests.clear()
    this.stopSweepIfIdle()
  }

  async fetch<T>(
    key: string,
    fetcher: () => Promise<T>,
    ttl?: number,
    forceRefresh = false,
  ): Promise<T> {
    const cached = forceRefresh ? null : this.get<T>(key)
    if (!forceRefresh && this.cache.has(key)) {
      return cached as T
    }

    const pending = this.pendingRequests.get(key)
    if (pending) {
      return pending as Promise<T>
    }

    const promise = fetcher()
      .then((data) => {
        if (this.pendingRequests.get(key) === promise) {
          this.set(key, data, ttl)
        }
        return data
      })
      .catch((error) => {
        if (this.pendingRequests.get(key) === promise) {
          this.pendingRequests.delete(key)
        }
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

  getStatus() {
    return {
      size: this.cache.size,
      maxEntries: this.maxEntries,
      pending: this.pendingRequests.size,
    }
  }
}

export const requestCache = new RequestCache()
