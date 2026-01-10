/**
 * useIdleCallback - 将非关键任务推迟到主线程空闲时执行
 * 用于优化 Lighthouse 的 "最大限度地减少主线程工作" 指标
 *
 * 使用场景：
 * - 初始化时的非关键任务（如预加载、统计上报）
 * - 轮询操作（如后台健康检查）
 * - 低优先级状态更新
 */

import { useEffect, useRef, useState } from 'react'

// 兼容 requestIdleCallback 的类型定义
interface IdleDeadline {
  didTimeout: boolean
  timeRemaining: () => number
}

type IdleRequestCallback = (deadline: IdleDeadline) => void

// Polyfill for browsers that don't support requestIdleCallback
function requestIdleCallbackPolyfill(callback: IdleRequestCallback, options?: { timeout?: number }): number {
  const start = Date.now()
  return window.setTimeout(() => {
    callback({
      didTimeout: options?.timeout ? Date.now() - start >= options.timeout : false,
      timeRemaining: () => Math.max(0, 50 - (Date.now() - start)),
    })
  }, 1) as unknown as number
}

function cancelIdleCallbackPolyfill(id: number): void {
  window.clearTimeout(id)
}

// 使用原生 API 或 polyfill
const requestIdle
  = typeof window !== 'undefined' && 'requestIdleCallback' in window
    ? (window as any).requestIdleCallback.bind(window) as typeof requestIdleCallbackPolyfill
    : requestIdleCallbackPolyfill

const cancelIdle
  = typeof window !== 'undefined' && 'cancelIdleCallback' in window
    ? (window as any).cancelIdleCallback.bind(window) as typeof cancelIdleCallbackPolyfill
    : cancelIdleCallbackPolyfill

/**
 * 在主线程空闲时执行一次性任务
 */
export function useIdleEffect(
  callback: () => void | (() => void),
  deps: React.DependencyList,
  options?: { timeout?: number },
) {
  useEffect(() => {
    let cleanup: (() => void) | void
    const idleId = requestIdle(
      () => {
        cleanup = callback()
      },
      options,
    )

    return () => {
      cancelIdle(idleId)
      if (typeof cleanup === 'function') {
        cleanup()
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps)
}

/**
 * 延迟执行的初始化 Hook
 * 比 setTimeout 更高效，在主线程空闲时执行
 */
export function useDeferredInit<T>(
  initFn: () => T,
  initialValue: T,
  options?: { timeout?: number },
): T {
  const [value, setValue] = useState<T>(initialValue)
  const hasInitRef = useRef(false)

  useEffect(() => {
    if (hasInitRef.current)
      return

    const idleId = requestIdle(
      () => {
        if (hasInitRef.current)
          return
        hasInitRef.current = true
        setValue(initFn())
      },
      options,
    )

    return () => {
      cancelIdle(idleId)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  return value
}

// 简单的强制更新 hook - 已被移除，直接使用 useState

/**
 * 批量执行空闲任务
 * 适用于需要分批处理大量数据的场景
 */
export function useIdleBatch<T>(
  items: T[],
  processFn: (item: T, index: number) => void,
  options?: {
    batchSize?: number
    timeout?: number
    enabled?: boolean
  },
) {
  const processedRef = useRef(0)
  const batchSize = options?.batchSize ?? 5
  const enabled = options?.enabled ?? true

  useEffect(() => {
    if (!enabled || items.length === 0)
      return

    processedRef.current = 0
    let cancelled = false

    const processNextBatch = () => {
      if (cancelled || processedRef.current >= items.length)
        return

      requestIdle(
        (deadline) => {
          if (cancelled)
            return

          // 在时间允许的范围内处理尽可能多的项目
          while (
            processedRef.current < items.length
            && (deadline.timeRemaining() > 5 || deadline.didTimeout)
          ) {
            const endIndex = Math.min(processedRef.current + batchSize, items.length)
            for (let i = processedRef.current; i < endIndex; i++) {
              processFn(items[i], i)
            }
            processedRef.current = endIndex
          }

          // 如果还有未处理的项目，继续调度
          if (processedRef.current < items.length) {
            processNextBatch()
          }
        },
        { timeout: options?.timeout ?? 100 },
      )
    }

    processNextBatch()

    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [items, enabled])
}

/**
 * 空闲定时器 - 替代 setInterval，在主线程空闲时执行
 * 更适合非关键性的轮询任务（如后台健康检查）
 */
export function useIdleInterval(
  callback: () => void,
  delay: number,
  options?: {
    enabled?: boolean
    pauseWhenHidden?: boolean // 页面隐藏时暂停
    timeout?: number // requestIdleCallback 超时时间
  },
) {
  const savedCallback = useRef(callback)
  const enabled = options?.enabled ?? true
  const pauseWhenHidden = options?.pauseWhenHidden ?? true

  // 更新回调引用
  useEffect(() => {
    savedCallback.current = callback
  }, [callback])

  useEffect(() => {
    if (!enabled)
      return

    let cancelled = false
    let timeoutId: ReturnType<typeof setTimeout> | null = null

    const tick = () => {
      if (cancelled)
        return

      // 页面隐藏时跳过执行
      if (pauseWhenHidden && document.hidden) {
        // 页面隐藏时延迟重试
        timeoutId = setTimeout(tick, delay)
        return
      }

      // 使用 requestIdleCallback 执行回调
      requestIdle(
        () => {
          if (cancelled)
            return
          savedCallback.current()
          // 调度下一次执行
          timeoutId = setTimeout(tick, delay)
        },
        { timeout: options?.timeout ?? Math.min(delay, 1000) },
      )
    }

    // 首次执行延迟
    timeoutId = setTimeout(tick, delay)

    // 可见性变化监听
    const handleVisibilityChange = () => {
      if (!document.hidden && timeoutId === null) {
        timeoutId = setTimeout(tick, delay)
      }
    }

    if (pauseWhenHidden) {
      document.addEventListener('visibilitychange', handleVisibilityChange)
    }

    return () => {
      cancelled = true
      if (timeoutId) {
        clearTimeout(timeoutId)
      }
      if (pauseWhenHidden) {
        document.removeEventListener('visibilitychange', handleVisibilityChange)
      }
    }
  }, [delay, enabled, pauseWhenHidden, options?.timeout])
}

export { cancelIdle, requestIdle }
