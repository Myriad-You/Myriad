import {
  startFpsMonitor as _startFpsMonitor,
  stopFpsMonitor as _stopFpsMonitor,
  isLowFps,
} from '../hooks/animation'

export function startFpsMonitor(): void {
  _startFpsMonitor()
}

export function stopFpsMonitor(): void {
  _stopFpsMonitor()
}

export function rafThrottle<T extends (...args: any[]) => any>(
  fn: T,
  options?: {
    skipOnLowFps?: boolean
  },
): ((...args: Parameters<T>) => void) & { cancel: () => void } {
  let rafId: number | null = null
  const { skipOnLowFps = false } = options ?? {}

  const throttled = function (this: any, ...args: Parameters<T>) {
    if (rafId !== null) {
      return
    }

    if (skipOnLowFps && isLowFps()) {
      return
    }

    rafId = requestAnimationFrame(() => {
      fn.call(this, ...args)
      rafId = null
    })
  } as ((...args: Parameters<T>) => void) & { cancel: () => void }

  throttled.cancel = () => {
    if (rafId !== null) {
      cancelAnimationFrame(rafId)
      rafId = null
    }
  }

  return throttled
}

export class MemoryManager {
  private static cache = new Map<string, any>()
  private static maxSize = 50

  static set(key: string, value: any): void {
    if (this.cache.size >= this.maxSize) {
      const firstKey = this.cache.keys().next().value
      if (firstKey !== undefined) {
        this.cache.delete(firstKey)
      }
    }

    this.cache.set(key, value)
  }

  static get(key: string): any {
    return this.cache.get(key)
  }

  static has(key: string): boolean {
    return this.cache.has(key)
  }

  static clear(): void {
    this.cache.clear()
  }

  static getSize(): number {
    return this.cache.size
  }
}
