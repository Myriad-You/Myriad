/** Conservative UTF-16 payload budget (shared references may be counted twice). */
function resourceBytes(value: unknown): number {
  if (typeof value === 'string') return value.length * 2
  if (!value || typeof value !== 'object') return 8
  return Object.entries(value).reduce((total, [key, entry]) => total + key.length * 2 + resourceBytes(entry), 0)
}

export class BoundedResourceCache<T> {
  private entries = new Map<string, { data: T; expires: number; bytes: number }>()
  private bytes = 0
  private timer: ReturnType<typeof setTimeout> | undefined
  constructor(private limit: number, private byteLimit: number) {}
  get size() { return this.entries.size }
  keys() { return this.entries.keys() }
  get(key: string): T | undefined {
    const entry = this.entries.get(key)
    if (!entry) return undefined
    if (entry.expires <= Date.now()) {
      this.delete(key)
      return undefined
    }
    this.entries.delete(key)
    this.entries.set(key, entry)
    return entry.data
  }

  set(key: string, data: T, ttl: number): void {
    this.delete(key)
    const bytes = resourceBytes(data)
    if (bytes > this.byteLimit) return
    this.entries.set(key, { data, bytes, expires: Date.now() + ttl })
    this.bytes += bytes
    while (this.size > this.limit || this.bytes > this.byteLimit) {
      this.delete(this.entries.keys().next().value!)
    }
    this.schedule()
  }

  delete(key: string): void {
    const entry = this.entries.get(key)
    if (!entry) return
    this.bytes -= entry.bytes
    this.entries.delete(key)
    this.schedule()
  }

  clear(): void {
    this.entries.clear()
    this.bytes = 0
    clearTimeout(this.timer)
    this.timer = undefined
  }

  private schedule(): void {
    clearTimeout(this.timer)
    this.timer = undefined
    if (!this.size) return
    const expires = Math.min(...Array.from(this.entries.values(), entry => entry.expires))
    this.timer = setTimeout(() => {
      const now = Date.now()
      for (const [key, entry] of this.entries) {
        if (entry.expires <= now) {
          this.bytes -= entry.bytes
          this.entries.delete(key)
        }
      }
      this.schedule()
    }, Math.max(1, expires - Date.now()))
    // Do not keep node-based tests / SSR alive for a browser cache.
    ;(this.timer as unknown as { unref?: () => void }).unref?.()
  }
}

/** Holds only eviction callbacks; iframes always stay in their original parent. */
export class HiddenSandboxPool {
  private hidden = new Map<object | string, () => void>()
  constructor(private capacity: number) {}
  add(key: object | string, evict: () => void): void {
    this.hidden.delete(key)
    this.hidden.set(key, evict)
    while (this.hidden.size > this.capacity) {
      const oldest = this.hidden.entries().next().value!
      this.hidden.delete(oldest[0])
      oldest[1]()
    }
  }

  remove(key: object | string): void { this.hidden.delete(key) }
}

export const hiddenWidgetPool = new HiddenSandboxPool(12)
