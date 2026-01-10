/**
 * 定时器管理 Hook
 *
 * 提供自动清理的定时器 API，避免组件卸载时的内存泄漏
 *
 * @module useTimers
 * @version 1.0
 */

import { useCallback, useEffect, useRef, useState } from 'react'

type TimerId = ReturnType<typeof setTimeout>
type IntervalId = ReturnType<typeof setInterval>
type RafId = ReturnType<typeof requestAnimationFrame>

interface TimerManager {
  /** 设置超时，组件卸载时自动清理 */
  setTimeout: (fn: () => void, ms: number) => TimerId
  /** 清除特定超时 */
  clearTimeout: (id: TimerId) => void
  /** 设置间隔，组件卸载时自动清理 */
  setInterval: (fn: () => void, ms: number) => IntervalId
  /** 清除特定间隔 */
  clearInterval: (id: IntervalId) => void
  /** 请求动画帧，组件卸载时自动清理 */
  requestAnimationFrame: (fn: FrameRequestCallback) => RafId
  /** 取消特定动画帧 */
  cancelAnimationFrame: (id: RafId) => void
  /** 清理所有定时器 */
  clearAll: () => void
}

/**
 * 自动管理定时器生命周期的 Hook
 *
 * @example
 * ```tsx
 * function MyComponent() {
 *   const timers = useTimers();
 *
 *   useEffect(() => {
 *     // 组件卸载时会自动清理
 *     timers.setTimeout(() => {
 *       console.log('Delayed action');
 *     }, 1000);
 *   }, [timers]);
 * }
 * ```
 */
export function useTimers(): TimerManager {
  const timeoutsRef = useRef<Set<TimerId>>(new Set())
  const intervalsRef = useRef<Set<IntervalId>>(new Set())
  const rafsRef = useRef<Set<RafId>>(new Set())

  // 清理所有定时器
  const clearAll = useCallback(() => {
    timeoutsRef.current.forEach(id => clearTimeout(id))
    timeoutsRef.current.clear()

    intervalsRef.current.forEach(id => clearInterval(id))
    intervalsRef.current.clear()

    rafsRef.current.forEach(id => cancelAnimationFrame(id))
    rafsRef.current.clear()
  }, [])

  // 组件卸载时清理
  useEffect(() => {
    return clearAll
  }, [clearAll])

  const safeSetTimeout = useCallback((fn: () => void, ms: number): TimerId => {
    const id = setTimeout(() => {
      timeoutsRef.current.delete(id)
      fn()
    }, ms)
    timeoutsRef.current.add(id)
    return id
  }, [])

  const safeClearTimeout = useCallback((id: TimerId) => {
    clearTimeout(id)
    timeoutsRef.current.delete(id)
  }, [])

  const safeSetInterval = useCallback((fn: () => void, ms: number): IntervalId => {
    const id = setInterval(fn, ms)
    intervalsRef.current.add(id)
    return id
  }, [])

  const safeClearInterval = useCallback((id: IntervalId) => {
    clearInterval(id)
    intervalsRef.current.delete(id)
  }, [])

  const safeRaf = useCallback((fn: FrameRequestCallback): RafId => {
    const id = requestAnimationFrame((time) => {
      rafsRef.current.delete(id)
      fn(time)
    })
    rafsRef.current.add(id)
    return id
  }, [])

  const safeCancelRaf = useCallback((id: RafId) => {
    cancelAnimationFrame(id)
    rafsRef.current.delete(id)
  }, [])

  // 使用 useRef 保持 manager 对象引用稳定
  const managerRef = useRef<TimerManager | null>(null)
  if (!managerRef.current) {
    managerRef.current = {
      setTimeout: safeSetTimeout,
      clearTimeout: safeClearTimeout,
      setInterval: safeSetInterval,
      clearInterval: safeClearInterval,
      requestAnimationFrame: safeRaf,
      cancelAnimationFrame: safeCancelRaf,
      clearAll,
    }
  }

  return managerRef.current
}

/**
 * 延迟执行 Hook
 * 在指定延迟后执行回调，自动处理清理
 *
 * @param callback 要执行的回调
 * @param delay 延迟毫秒数，null 时不执行
 *
 * @example
 * ```tsx
 * useTimeout(() => {
 *   setVisible(false);
 * }, isVisible ? 3000 : null);
 * ```
 */
export function useTimeout(callback: () => void, delay: number | null): void {
  const savedCallback = useRef(callback)

  // 保存最新的回调
  useEffect(() => {
    savedCallback.current = callback
  }, [callback])

  useEffect(() => {
    if (delay === null)
      return

    const id = setTimeout(() => {
      savedCallback.current()
    }, delay)

    return () => clearTimeout(id)
  }, [delay])
}

/**
 * 间隔执行 Hook
 * 按指定间隔执行回调，自动处理清理
 *
 * @param callback 要执行的回调
 * @param delay 间隔毫秒数，null 时暂停
 *
 * @example
 * ```tsx
 * useInterval(() => {
 *   setCount(c => c + 1);
 * }, isRunning ? 1000 : null);
 * ```
 */
export function useInterval(callback: () => void, delay: number | null): void {
  const savedCallback = useRef(callback)

  useEffect(() => {
    savedCallback.current = callback
  }, [callback])

  useEffect(() => {
    if (delay === null)
      return

    const id = setInterval(() => {
      savedCallback.current()
    }, delay)

    return () => clearInterval(id)
  }, [delay])
}

/**
 * 防抖 Hook
 * 返回防抖后的值，在指定延迟后更新
 *
 * @param value 要防抖的值
 * @param delay 防抖延迟毫秒数
 *
 * @example
 * ```tsx
 * const [text, setText] = useState('');
 * const debouncedText = useDebounce(text, 500);
 *
 * useEffect(() => {
 *   // 只在用户停止输入 500ms 后执行搜索
 *   search(debouncedText);
 * }, [debouncedText]);
 * ```
 */
export function useDebounce<T>(value: T, delay: number): T {
  const [debouncedValue, setDebouncedValue] = useState(value)

  useEffect(() => {
    const id = setTimeout(() => {
      setDebouncedValue(value)
    }, delay)

    return () => clearTimeout(id)
  }, [value, delay])

  return debouncedValue
}
