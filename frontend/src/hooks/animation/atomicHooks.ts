/**
 * 原子化 Hooks - 基于轻量级核心
 *
 * 每个 Hook 独立工作，只引入必要的核心功能
 *
 * @module animation/atomicHooks
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import {
  batchRead,
  batchWrite,
  isPageVisible,
  now,
  observeIntersection,
  observeResize,
  onVisibility,
  scheduleIdle,
} from './core'

// ==================== 可见性 Hooks ====================

/**
 * 页面可见性 Hook
 *
 * @example
 * ```tsx
 * const visible = usePageVisible();
 * if (!visible) pauseAnimation();
 * ```
 */
export function usePageVisible(): boolean {
  const [visible, setVisible] = useState(isPageVisible)

  useEffect(() => {
    return onVisibility(setVisible)
  }, [])

  return visible
}

/**
 * 可见性感知的 Interval
 * 页面隐藏时自动暂停
 *
 * @example
 * ```tsx
 * useVisibilityInterval(() => {
 *   fetchData();
 * }, { delay: 30000 });
 * ```
 */
export function useVisibilityInterval(
  callback: () => void,
  options: { delay: number, enabled?: boolean, immediate?: boolean },
): void {
  const { delay, enabled = true, immediate = false } = options
  const callbackRef = useRef(callback)
  const timeoutRef = useRef<number | null>(null)

  useEffect(() => {
    callbackRef.current = callback
  }, [callback])

  useEffect(() => {
    if (!enabled)
      return

    let cancelled = false

    const tick = () => {
      if (cancelled || !isPageVisible())
        return
      callbackRef.current()
      timeoutRef.current = window.setTimeout(tick, delay)
    }

    // 立即执行一次
    if (immediate && isPageVisible()) {
      callbackRef.current()
    }

    // 开始定时
    timeoutRef.current = window.setTimeout(tick, delay)

    // 订阅可见性
    const unsub = onVisibility((vis) => {
      if (vis && timeoutRef.current === null && !cancelled) {
        timeoutRef.current = window.setTimeout(tick, delay)
      }
      else if (!vis && timeoutRef.current !== null) {
        clearTimeout(timeoutRef.current)
        timeoutRef.current = null
      }
    })

    return () => {
      cancelled = true
      unsub()
      if (timeoutRef.current !== null) {
        clearTimeout(timeoutRef.current)
      }
    }
  }, [delay, enabled, immediate])
}

// ==================== 尺寸观察 Hook ====================

/**
 * 元素尺寸观察 Hook
 *
 * @example
 * ```tsx
 * const { ref, width, height } = useElementSize<HTMLDivElement>();
 * return <div ref={ref}>Size: {width}x{height}</div>;
 * ```
 */
export function useElementSize<T extends Element>(): {
  ref: React.RefCallback<T>
  width: number
  height: number
} {
  const [size, setSize] = useState({ width: 0, height: 0 })
  const unobserveRef = useRef<(() => void) | null>(null)

  const ref = useCallback((element: T | null) => {
    // 清理旧观察
    if (unobserveRef.current) {
      unobserveRef.current()
      unobserveRef.current = null
    }

    if (element) {
      unobserveRef.current = observeResize(element, (entry) => {
        const { width, height } = entry.contentRect
        setSize((prev) => {
          if (prev.width === width && prev.height === height)
            return prev
          return { width, height }
        })
      })
    }
  }, [])

  // 清理
  useEffect(() => {
    return () => {
      if (unobserveRef.current) {
        unobserveRef.current()
      }
    }
  }, [])

  return { ref, ...size }
}

// ==================== 可见性观察 Hook ====================

/**
 * 元素视口可见性 Hook
 *
 * @example
 * ```tsx
 * const { ref, isVisible } = useInView<HTMLDivElement>({ threshold: 0.5 });
 * return <div ref={ref}>{isVisible ? 'Visible' : 'Hidden'}</div>;
 * ```
 */
export function useInView<T extends Element>(options?: {
  threshold?: number
  rootMargin?: string
  once?: boolean
}): {
  ref: React.RefCallback<T>
  isVisible: boolean
} {
  const { threshold = 0, rootMargin = '0px', once = false } = options ?? {}
  const [isVisible, setIsVisible] = useState(false)
  const unobserveRef = useRef<(() => void) | null>(null)
  const hasTriggeredRef = useRef(false)

  const ref = useCallback((element: T | null) => {
    if (unobserveRef.current) {
      unobserveRef.current()
      unobserveRef.current = null
    }

    if (element && !(once && hasTriggeredRef.current)) {
      unobserveRef.current = observeIntersection(
        element,
        (entry) => {
          const visible = entry.isIntersecting
          setIsVisible(visible)

          if (visible && once) {
            hasTriggeredRef.current = true
            unobserveRef.current?.()
            unobserveRef.current = null
          }
        },
        { threshold, rootMargin },
      )
    }
  }, [threshold, rootMargin, once])

  useEffect(() => {
    return () => {
      if (unobserveRef.current) {
        unobserveRef.current()
      }
    }
  }, [])

  return { ref, isVisible }
}

/**
 * 懒加载图片 Hook
 *
 * @example
 * ```tsx
 * const { ref, shouldLoad } = useLazyLoad<HTMLImageElement>();
 * return <img ref={ref} src={shouldLoad ? src : placeholder} />;
 * ```
 */
export function useLazyLoad<T extends Element>(rootMargin = '200px'): {
  ref: React.RefCallback<T>
  shouldLoad: boolean
} {
  const { ref, isVisible } = useInView<T>({
    rootMargin,
    once: true,
  })

  return { ref, shouldLoad: isVisible }
}

// ==================== 空闲任务 Hook ====================

/**
 * 空闲时执行 Hook
 *
 * @example
 * ```tsx
 * useIdleEffect(() => {
 *   prefetchNextPage();
 * }, ['prefetch-page']);
 * ```
 */
export function useIdleEffect(
  callback: () => void,
  deps: React.DependencyList,
  options?: { priority?: 'low' | 'normal' | 'high' },
): void {
  const callbackRef = useRef(callback)
  callbackRef.current = callback

  useEffect(() => {
    const id = `idle-${now()}-${Math.random().toString(36).slice(2, 9)}`
    const cancel = scheduleIdle(id, () => callbackRef.current(), options?.priority)
    return cancel
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps)
}

// ==================== DOM 批量操作 Hooks ====================

/**
 * 批量 DOM 操作 Hook
 *
 * @example
 * ```tsx
 * const { measureElement, updateElement } = useBatchedDom();
 *
 * measureElement(() => {
 *   const rect = el.getBoundingClientRect();
 *   // ...
 * });
 *
 * updateElement(() => {
 *   el.style.transform = `translateX(${x}px)`;
 * });
 * ```
 */
export function useBatchedDom(): {
  measureElement: (callback: () => void) => void
  updateElement: (callback: () => void) => void
} {
  return {
    measureElement: batchRead,
    updateElement: batchWrite,
  }
}

// ==================== 动画帧 Hook ====================

/**
 * RAF 循环 Hook
 *
 * @example
 * ```tsx
 * useAnimationFrame((deltaTime) => {
 *   position += velocity * deltaTime;
 * }, { enabled: isAnimating });
 * ```
 */
export function useAnimationFrame(
  callback: (deltaTime: number) => void,
  options?: { enabled?: boolean },
): void {
  const { enabled = true } = options ?? {}
  const callbackRef = useRef(callback)
  const lastTimeRef = useRef(0)

  useEffect(() => {
    callbackRef.current = callback
  }, [callback])

  useEffect(() => {
    if (!enabled)
      return

    let rafId: number

    const tick = (time: number) => {
      if (lastTimeRef.current === 0) {
        lastTimeRef.current = time
      }
      const delta = time - lastTimeRef.current
      lastTimeRef.current = time

      callbackRef.current(delta)
      rafId = requestAnimationFrame(tick)
    }

    rafId = requestAnimationFrame(tick)

    return () => {
      cancelAnimationFrame(rafId)
      lastTimeRef.current = 0
    }
  }, [enabled])
}

// ==================== 防抖节流 Hooks ====================

/**
 * 节流回调 Hook
 *
 * @example
 * ```tsx
 * const throttledScroll = useThrottle((e) => {
 *   console.log(e.scrollTop);
 * }, 100);
 * ```
 */
export function useThrottle<T extends (...args: any[]) => void>(
  callback: T,
  ms: number,
): T {
  const lastRunRef = useRef(0)
  const callbackRef = useRef(callback)

  useEffect(() => {
    callbackRef.current = callback
  }, [callback])

  return useCallback((...args: Parameters<T>) => {
    const nowTime = now()
    if (nowTime - lastRunRef.current >= ms) {
      lastRunRef.current = nowTime
      callbackRef.current(...args)
    }
  }, [ms]) as T
}

/**
 * 防抖值 Hook
 *
 * @example
 * ```tsx
 * const [search, setSearch] = useState('');
 * const debouncedSearch = useDebounce(search, 300);
 * ```
 */
export function useDebounce<T>(value: T, ms: number): T {
  const [debounced, setDebounced] = useState(value)

  useEffect(() => {
    const timer = setTimeout(() => setDebounced(value), ms)
    return () => clearTimeout(timer)
  }, [value, ms])

  return debounced
}
