/**
 * IndexedDB缓存管理工具
 * 用于离线存储和大数据缓存
 */

interface CacheEntry<T> {
  data: T
  timestamp: number
  expiry?: number // 过期时间(ms)
}

export class IndexedDBCache {
  private dbName: string
  private storeName: string
  private db: IDBDatabase | null = null

  constructor(dbName: string = 'myriad-cache', storeName: string = 'data') {
    this.dbName = dbName
    this.storeName = storeName
  }

  /**
   * 初始化数据库
   */
  async init(): Promise<void> {
    if (!('indexedDB' in window)) {
      console.warn('IndexedDB not supported')
      return
    }

    return new Promise((resolve, reject) => {
      const request = indexedDB.open(this.dbName, 1)

      request.onerror = () => reject(request.error)
      request.onsuccess = () => {
        this.db = request.result
        resolve()
      }

      request.onupgradeneeded = (event) => {
        const db = (event.target as IDBOpenDBRequest).result

        if (!db.objectStoreNames.contains(this.storeName)) {
          const objectStore = db.createObjectStore(this.storeName, { keyPath: 'key' })
          objectStore.createIndex('timestamp', 'timestamp', { unique: false })
        }
      }
    })
  }

  /**
   * 设置缓存
   */
  async set<T>(key: string, data: T, expiryMs?: number): Promise<void> {
    if (!this.db)
      await this.init()
    if (!this.db)
      throw new Error('Database not initialized')

    const entry: CacheEntry<T> = {
      data,
      timestamp: Date.now(),
      expiry: expiryMs,
    }

    return new Promise((resolve, reject) => {
      const transaction = this.db!.transaction([this.storeName], 'readwrite')
      const store = transaction.objectStore(this.storeName)
      const request = store.put({ key, ...entry })

      request.onsuccess = () => resolve()
      request.onerror = () => reject(request.error)
    })
  }

  /**
   * 获取缓存
   */
  async get<T>(key: string): Promise<T | null> {
    if (!this.db)
      await this.init()
    if (!this.db)
      return null

    return new Promise((resolve, reject) => {
      const transaction = this.db!.transaction([this.storeName], 'readonly')
      const store = transaction.objectStore(this.storeName)
      const request = store.get(key)

      request.onsuccess = () => {
        const result = request.result

        if (!result) {
          resolve(null)
          return
        }

        const entry = result as CacheEntry<T>

        // 检查是否过期
        if (entry.expiry) {
          const age = Date.now() - entry.timestamp
          if (age > entry.expiry) {
            // 过期，删除并返回null
            this.delete(key)
            resolve(null)
            return
          }
        }

        resolve(entry.data)
      }

      request.onerror = () => reject(request.error)
    })
  }

  /**
   * 删除缓存
   */
  async delete(key: string): Promise<void> {
    if (!this.db)
      await this.init()
    if (!this.db)
      return

    return new Promise((resolve, reject) => {
      const transaction = this.db!.transaction([this.storeName], 'readwrite')
      const store = transaction.objectStore(this.storeName)
      const request = store.delete(key)

      request.onsuccess = () => resolve()
      request.onerror = () => reject(request.error)
    })
  }

  /**
   * 清空所有缓存
   */
  async clear(): Promise<void> {
    if (!this.db)
      await this.init()
    if (!this.db)
      return

    return new Promise((resolve, reject) => {
      const transaction = this.db!.transaction([this.storeName], 'readwrite')
      const store = transaction.objectStore(this.storeName)
      const request = store.clear()

      request.onsuccess = () => resolve()
      request.onerror = () => reject(request.error)
    })
  }

  /**
   * 获取所有键
   */
  async keys(): Promise<string[]> {
    if (!this.db)
      await this.init()
    if (!this.db)
      return []

    return new Promise((resolve, reject) => {
      const transaction = this.db!.transaction([this.storeName], 'readonly')
      const store = transaction.objectStore(this.storeName)
      const request = store.getAllKeys()

      request.onsuccess = () => resolve(request.result as string[])
      request.onerror = () => reject(request.error)
    })
  }

  /**
   * 清理过期缓存
   */
  async cleanupExpired(): Promise<number> {
    if (!this.db)
      await this.init()
    if (!this.db)
      return 0

    const keys = await this.keys()
    let cleaned = 0

    for (const key of keys) {
      const data = await this.get(key)
      if (data === null) {
        cleaned++
      }
    }

    return cleaned
  }
}

// 全局实例
export const globalCache = new IndexedDBCache()

// 自动清理过期缓存(每小时)
if (typeof window !== 'undefined') {
  setInterval(() => {
    globalCache.cleanupExpired()
  }, 60 * 60 * 1000)
}
