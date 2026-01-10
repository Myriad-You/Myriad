/**
 * useVisibilityPause - 页面可见性感知的定时器 Hook
 *
 * 解决问题：
 * 许多组件需要定时器，但在页面隐藏时应该暂停以节省资源
 * 之前每个组件都各自监听 visibilitychange 事件，造成大量重复代码和监听器
 *
 * 功能特性：
 * - 页面隐藏时自动暂停定时器
 * - 页面可见时自动恢复
 * - 使用统一的可见性管理器，减少监听器数量
 * - 支持 interval 和 timeout 两种模式
 */

import { useCallback, useEffect, useRef } from 'react'
import { coordinator } from './coordinator'

interface UseVisibilityIntervalOptions {
  /** 定时器间隔（ms） */
  delay: number
  /** 是否启用 */
  enabled?: boolean
  /** 是否立即执行一次 */
  immediate?: boolean
}

/**
 * 页面可见性感知的 setInterval
 * 页面隐藏时自动暂停，可见时自动恢复
 *
 * @param callback 定时执行的回调
 * @param options 配置选项
 *
 * @example
 * ```tsx
 * // 30分钟刷新天气数据，页面隐藏时暂停
 * useVisibilityInterval(() => {
 *   fetchWeather();
 * }, { delay: 30 * 60 * 1000, enabled: !isPreview });
 * ```
 */
export function useVisibilityInterval(
  callback: () => void,
  options: UseVisibilityIntervalOptions,
) {
  const { delay, enabled = true, immediate = false } = options
  const savedCallback = useRef(callback)
  const timeoutIdRef = useRef<number | null>(null)
  const cancelledRef = useRef(false)

  // 更新回调引用
  useEffect(() => {
    savedCallback.current = callback
  }, [callback])

  // 清理定时器的函数
  const clearTimer = useCallback(() => {
    if (timeoutIdRef.current !== null) {
      clearTimeout(timeoutIdRef.current)
      timeoutIdRef.current = null
    }
  }, [])

  // 调度下一次执行
  const scheduleNext = useCallback(() => {
    if (cancelledRef.current || !coordinator.getPageVisibility())
      return

    timeoutIdRef.current = window.setTimeout(() => {
      if (cancelledRef.current || !coordinator.getPageVisibility())
        return
      savedCallback.current()
      scheduleNext()
    }, delay)
  }, [delay])

  useEffect(() => {
    if (!enabled)
      return

    cancelledRef.current = false

    // 立即执行一次
    if (immediate && coordinator.getPageVisibility()) {
      savedCallback.current()
    }

    // 开始定时
    scheduleNext()

    // 订阅可见性变化
    const unsubscribe = coordinator.onVisibilityChange((isVisible) => {
      if (isVisible) {
        // 页面变为可见，恢复定时
        if (timeoutIdRef.current === null && !cancelledRef.current) {
          scheduleNext()
        }
      }
      else {
        // 页面隐藏，暂停定时
        clearTimer()
      }
    })

    return () => {
      cancelledRef.current = true
      clearTimer()
      unsubscribe()
    }
  }, [enabled, delay, immediate, scheduleNext, clearTimer])
}

interface UseVisibilityTimeoutOptions {
  /** 延迟时间（ms） */
  delay: number
  /** 是否启用 */
  enabled?: boolean
}

/**
 * 页面可见性感知的 setTimeout
 * 页面隐藏时暂停计时，可见时继续
 *
 * @param callback 延迟执行的回调
 * @param options 配置选项
 * @returns 取消定时器的函数
 *
 * @example
 * ```tsx
 * // 5秒后切换内容，页面隐藏时暂停计时
 * useVisibilityTimeout(() => {
 *   setCurrentIndex((prev) => (prev + 1) % items.length);
 * }, { delay: 5000 });
 * ```
 */
export function useVisibilityTimeout(
  callback: () => void,
  options: UseVisibilityTimeoutOptions,
) {
  const { delay, enabled = true } = options
  const savedCallback = useRef(callback)
  const timeoutIdRef = useRef<number | null>(null)
  const startTimeRef = useRef<number>(0)
  const remainingTimeRef = useRef<number>(delay)
  const hasExecutedRef = useRef(false)

  // 更新回调引用
  useEffect(() => {
    savedCallback.current = callback
  }, [callback])

  useEffect(() => {
    if (!enabled)
      return

    hasExecutedRef.current = false
    remainingTimeRef.current = delay

    const startTimer = () => {
      if (hasExecutedRef.current)
        return

      startTimeRef.current = Date.now()
      timeoutIdRef.current = window.setTimeout(() => {
        if (!hasExecutedRef.current) {
          hasExecutedRef.current = true
          savedCallback.current()
        }
      }, remainingTimeRef.current)
    }

    const pauseTimer = () => {
      if (timeoutIdRef.current !== null) {
        clearTimeout(timeoutIdRef.current)
        timeoutIdRef.current = null
        // 计算剩余时间
        const elapsed = Date.now() - startTimeRef.current
        remainingTimeRef.current = Math.max(0, remainingTimeRef.current - elapsed)
      }
    }

    // 如果页面可见，开始计时
    if (coordinator.getPageVisibility()) {
      startTimer()
    }

    // 订阅可见性变化
    const unsubscribe = coordinator.onVisibilityChange((isVisible) => {
      if (isVisible) {
        startTimer()
      }
      else {
        pauseTimer()
      }
    })

    return () => {
      if (timeoutIdRef.current !== null) {
        clearTimeout(timeoutIdRef.current)
      }
      unsubscribe()
    }
  }, [enabled, delay])
}

// 注意: usePageVisible 已移至 useSharedEventListener.ts 中统一导出为 usePageVisibility
// 为保持向后兼容，从这里重新导出
export { usePageVisibility as usePageVisible } from '../useSharedEventListener'
