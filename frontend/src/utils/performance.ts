import {
  startFpsMonitor as _startFpsMonitor,
  stopFpsMonitor as _stopFpsMonitor,
  batchRead,
  batchWrite,
  isLowFps,
  shouldYield,
  yieldToMain,
} from '../hooks/animation'

import { loadImagePooled } from './objectPool'

/**
 * 性能优化工具函数库
 * 提供防抖、节流、RAF优化等性能工具
 */

// 帧率感知 - 代理到 AnimationCoordinator

/** 检查是否处于低帧率模式 */
export function isLowFpsMode(): boolean {
  return isLowFps()
}

/** 启动 FPS 监控（自动降级） */
export function startFpsMonitor(): void {
  _startFpsMonitor()
}

/** 停止 FPS 监控 */
export function stopFpsMonitor(): void {
  _stopFpsMonitor()
}

// 重新导出 DOM 批量操作
export { batchRead, batchWrite, shouldYield, yieldToMain }

/**
 * 防抖函数 - 延迟执行
 * @param fn 要防抖的函数
 * @param delay 延迟时间(ms)
 */
export function debounce<T extends (...args: any[]) => any>(
  fn: T,
  delay: number = 300,
): (...args: Parameters<T>) => void {
  let timeoutId: ReturnType<typeof setTimeout> | null = null

  return function (this: any, ...args: Parameters<T>) {
    if (timeoutId) {
      clearTimeout(timeoutId)
    }

    timeoutId = setTimeout(() => {
      fn.apply(this, args)
      timeoutId = null
    }, delay)
  }
}

/**
 * 节流函数 - 限制执行频率
 * @param fn 要节流的函数
 * @param limit 时间限制(ms)
 */
export function throttle<T extends (...args: any[]) => any>(
  fn: T,
  limit: number = 300,
): (...args: Parameters<T>) => void {
  let inThrottle: boolean = false
  let lastResult: ReturnType<T>

  return function (this: any, ...args: Parameters<T>) {
    if (!inThrottle) {
      inThrottle = true
      lastResult = fn.apply(this, args)

      setTimeout(() => {
        inThrottle = false
      }, limit)
    }

    return lastResult
  }
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
 * 双缓冲 RAF - 确保平滑更新
 * 使用两个 RAF 确保样式更改在下一帧生效
 */
export function doubleRaf(callback: () => void): void {
  requestAnimationFrame(() => {
    requestAnimationFrame(callback)
  })
}

/**
 * 空闲执行 - 使用requestIdleCallback
 * @param fn 要执行的函数
 * @param options 配置选项
 */
export function runWhenIdle(
  fn: () => void,
  options?: { timeout?: number },
): void {
  if ('requestIdleCallback' in window) {
    requestIdleCallback(fn, options)
  } else {
    // 降级为setTimeout
    setTimeout(fn, 1)
  }
}

/**
 * 预加载图片（使用对象池）
 * @param src 图片URL
 */
export async function preloadImage(src: string): Promise<void> {
  const success = await loadImagePooled(src)
  if (!success) {
    throw new Error(`Failed to preload image: ${src}`)
  }
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
