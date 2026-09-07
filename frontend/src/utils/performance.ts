import {
  startFpsMonitor as _startFpsMonitor,
  stopFpsMonitor as _stopFpsMonitor,
  isLowFps,
} from '../hooks/animation'

/**
 * 性能优化工具：FPS 监控、RAF 节流、进程内 LRU 缓存。
 */

/** 启动 FPS 监控（自动降级） */
export function startFpsMonitor(): void {
  _startFpsMonitor()
}

/** 停止 FPS 监控 */
export function stopFpsMonitor(): void {
  _stopFpsMonitor()
}

/**
 * RAF节流 - 使用requestAnimationFrame限制执行
 * 增强版：支持取消和帧率感知
 *
 * @param fn 要优化的函数
 * @param options 选项
 */
export function rafThrottle<T extends (...args: any[]) => any>(
  fn: T,
  options?: {
    /** 低帧率时是否跳过执行 */
    skipOnLowFps?: boolean
  },
): ((...args: Parameters<T>) => void) & { cancel: () => void } {
  let rafId: number | null = null
  const { skipOnLowFps = false } = options || {}

  const throttled = function (this: any, ...args: Parameters<T>) {
    if (rafId !== null) {
      return
    }

    // 修复：使用正确的函数调用而非未定义变量
    if (skipOnLowFps && isLowFps()) {
      return
    }

    rafId = requestAnimationFrame(() => {
      fn.apply(this, args)
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

/**
 * 内存管理 - 清理未使用的对象
 */
export class MemoryManager {
  private static cache = new Map<string, any>()
  private static maxSize = 50

  static set(key: string, value: any): void {
    // LRU策略 - 超过限制删除最早的
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
