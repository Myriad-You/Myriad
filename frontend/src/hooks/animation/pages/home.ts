/**
 * 首页专用调度器 Hooks
 *
 * 首页功能需求：
 * - Visibility: Widget 可见性感知（暂停后台动画）
 * - Resize: WidgetGrid 响应式布局
 * - RAF: 拖拽动画节流
 * - Idle: 预加载、低优先级任务
 *
 * @example
 * ```tsx
 * // 在 Home.tsx 中
 * import { useHomeScheduler, useHomeResize, useHomeRaf } from '@hooks/animation/pages/home';
 *
 * function Home() {
 *   useHomeScheduler(); // 初始化首页调度器
 *   return <WidgetGrid />;
 * }
 *
 * function WidgetGrid() {
 *   const { width, height } = useHomeResize(containerRef);
 *   // ...
 * }
 * ```
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import { isPageVisible, onVisibility } from '../core'
import { Feature, hasFeature } from '../pageFeatures'

const PAGE_ID = 'home'

// ==================== 页面初始化 ====================

/**
 * 首页调度器初始化
 * 在 Home.tsx 顶层调用
 *
 * 注意：startPage('home') 由 useRouteScheduler 统一调用
 */
export function useHomeScheduler(): void {
  useEffect(() => {
    return () => cleanupHome()
  }, [])
}

// ==================== 可见性 Hooks ====================

/**
 * 首页可见性感知 Hook
 * 用于暂停后台动画、轮播等
 */
export function useHomeVisibility(): boolean {
  const [visible, setVisible] = useState(() => isPageVisible())

  useEffect(() => {
    if (!hasFeature(PAGE_ID, Feature.Visibility)) {
      console.warn('[Home] Visibility feature not enabled')
      return
    }
    return onVisibility(setVisible)
  }, [])

  return visible
}

// 活跃的 interval 追踪
const _homeIntervals = new Set<ReturnType<typeof setInterval>>()

/**
 * 首页可见性感知定时器
 * 页面隐藏时自动暂停，可见时自动恢复
 *
 * @example
 * ```tsx
 * useHomeVisibilityInterval(() => {
 *   setCurrentIndex(prev => (prev + 1) % items.length);
 * }, 5000);
 * ```
 */
export function useHomeVisibilityInterval(
  callback: () => void,
  delay: number,
  enabled = true,
): void {
  const savedCallback = useRef(callback)
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const visible = useHomeVisibility()

  useEffect(() => {
    savedCallback.current = callback
  }, [callback])

  useEffect(() => {
    if (!enabled)
      return

    if (visible) {
      intervalRef.current = setInterval(() => {
        savedCallback.current()
      }, delay)
      _homeIntervals.add(intervalRef.current)
    }

    return () => {
      if (intervalRef.current !== null) {
        clearInterval(intervalRef.current)
        _homeIntervals.delete(intervalRef.current)
        intervalRef.current = null
      }
    }
  }, [delay, visible, enabled])
}

// ==================== Resize Hooks ====================

// 共享的 ResizeObserver（首页内复用）
let _homeResizeObserver: ResizeObserver | null = null
const _homeResizeCallbacks = new Map<Element, (entry: ResizeObserverEntry) => void>()

function getHomeResizeObserver(): ResizeObserver {
  if (!_homeResizeObserver) {
    _homeResizeObserver = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const cb = _homeResizeCallbacks.get(entry.target)
        if (cb)
          cb(entry)
      }
    })
  }
  return _homeResizeObserver
}

/**
 * 首页元素尺寸监听
 * 用于 WidgetGrid 响应式布局
 */
export function useHomeResize<T extends Element>(
  ref: React.RefObject<T>,
): { width: number, height: number } {
  const [size, setSize] = useState({ width: 0, height: 0 })

  useEffect(() => {
    if (!hasFeature(PAGE_ID, Feature.Resize)) {
      console.warn('[Home] Resize feature not enabled')
      return
    }

    const el = ref.current
    if (!el)
      return

    const observer = getHomeResizeObserver()
    const callback = (entry: ResizeObserverEntry) => {
      const { width, height } = entry.contentRect
      setSize((prev) => {
        // 避免不必要的更新
        if (Math.abs(prev.width - width) < 1 && Math.abs(prev.height - height) < 1) {
          return prev
        }
        return { width, height }
      })
    }

    _homeResizeCallbacks.set(el, callback)
    observer.observe(el)

    // 立即测量
    const rect = el.getBoundingClientRect()
    setSize({ width: rect.width, height: rect.height })

    return () => {
      _homeResizeCallbacks.delete(el)
      observer.unobserve(el)
    }
  }, [ref])

  return size
}

/**
 * 首页元素尺寸监听（命令式 API）
 * 用于 callback ref 场景
 *
 * @example
 * ```tsx
 * const { observeHomeResize, unobserveHomeResize } = useHomeResizeObserver();
 * const containerRef = useCallback((node: HTMLDivElement | null) => {
 *   if (node) observeHomeResize(node, (entry) => setWidth(entry.contentRect.width));
 * }, []);
 * ```
 */
export function useHomeResizeObserver(): {
  observeHomeResize: (el: Element, callback: (entry: ResizeObserverEntry) => void) => void
  unobserveHomeResize: (el: Element) => void
} {
  const observeHomeResize = useCallback((el: Element, callback: (entry: ResizeObserverEntry) => void) => {
    if (!hasFeature(PAGE_ID, Feature.Resize)) {
      return
    }
    const observer = getHomeResizeObserver()
    _homeResizeCallbacks.set(el, callback)
    observer.observe(el)
    // 立即触发一次
    const rect = el.getBoundingClientRect()
    callback({ contentRect: rect } as ResizeObserverEntry)
  }, [])

  const unobserveHomeResize = useCallback((el: Element) => {
    _homeResizeCallbacks.delete(el)
    if (_homeResizeObserver) {
      _homeResizeObserver.unobserve(el)
    }
  }, [])

  return { observeHomeResize, unobserveHomeResize }
}

// ==================== RAF Hooks ====================

/**
 * 首页 RAF 节流
 * 用于拖拽动画等高频操作
 */
export function useHomeRaf<T extends (...args: any[]) => void>(
  callback: T,
  deps: React.DependencyList = [],
): T {
  const rafId = useRef<number | null>(null)
  const lastArgs = useRef<any[]>([])

  const throttled = useCallback((...args: any[]) => {
    if (!hasFeature(PAGE_ID, Feature.RAF)) {
      // 功能未启用，直接调用
      callback(...args)
      return
    }

    lastArgs.current = args
    if (rafId.current === null) {
      rafId.current = requestAnimationFrame(() => {
        rafId.current = null
        callback(...lastArgs.current)
      })
    }
  }, deps) as T

  useEffect(() => {
    return () => {
      if (rafId.current !== null) {
        cancelAnimationFrame(rafId.current)
      }
    }
  }, [])

  return throttled
}

// ==================== Idle Hooks ====================

/**
 * 首页空闲任务
 * 用于预加载图片、预取数据等
 */
export function useHomeIdle(
  callback: () => void,
  deps: React.DependencyList = [],
): void {
  useEffect(() => {
    if (!hasFeature(PAGE_ID, Feature.Idle)) {
      return
    }

    const id = requestIdleCallback(() => {
      if (isPageVisible()) {
        callback()
      }
    }, { timeout: 3000 })

    return () => cancelIdleCallback(id)
  }, deps)
}

// ==================== 清理 ====================

/**
 * 清理首页资源（路由离开时自动调用）
 */
export function cleanupHome(): void {
  // 清理所有 interval
  for (const id of _homeIntervals) {
    clearInterval(id)
  }
  _homeIntervals.clear()

  if (_homeResizeObserver) {
    _homeResizeObserver.disconnect()
    _homeResizeObserver = null
  }
  _homeResizeCallbacks.clear()
}
