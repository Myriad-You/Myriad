/**
 * 资料库页专用调度器 Hooks
 *
 * 资料库功能需求：
 * - Resize: 响应式网格布局
 * - Intersection: 无限滚动懒加载
 * - Idle: 预加载下一页数据
 *
 * @example
 * ```tsx
 * // 在 Library.tsx 中
 * import { useLibraryScheduler, useLibraryInView, useLibraryResize } from '@hooks/animation/pages/library';
 *
 * function Library() {
 *   useLibraryScheduler();
 *   return <LibraryGrid />;
 * }
 *
 * function LibraryItem({ item }) {
 *   const { ref, isInView } = useLibraryInView();
 *   return (
 *     <div ref={ref}>
 *       {isInView && <img src={item.cover} />}
 *     </div>
 *   );
 * }
 * ```
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import { isPageVisible } from '../core'
import { Feature, hasFeature } from '../pageFeatures'

const PAGE_ID = 'library'

// ==================== 页面初始化 ====================

/**
 * 资料库调度器初始化
 *
 * 注意：startPage('library') 由 useRouteScheduler 统一调用
 */
export function useLibraryScheduler(): void {
  useEffect(() => {
    return () => cleanupLibrary()
  }, [])
}

// ==================== Resize Hooks ====================

let _libraryResizeObserver: ResizeObserver | null = null
const _libraryResizeCallbacks = new Map<Element, (entry: ResizeObserverEntry) => void>()

function getLibraryResizeObserver(): ResizeObserver {
  if (!_libraryResizeObserver) {
    _libraryResizeObserver = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const cb = _libraryResizeCallbacks.get(entry.target)
        if (cb)
          cb(entry)
      }
    })
  }
  return _libraryResizeObserver
}

/**
 * 资料库元素尺寸监听
 */
export function useLibraryResize<T extends Element>(
  ref: React.RefObject<T>,
): { width: number, height: number } {
  const [size, setSize] = useState({ width: 0, height: 0 })

  useEffect(() => {
    if (!hasFeature(PAGE_ID, Feature.Resize)) {
      return
    }

    const el = ref.current
    if (!el)
      return

    const observer = getLibraryResizeObserver()
    const callback = (entry: ResizeObserverEntry) => {
      const { width, height } = entry.contentRect
      setSize((prev) => {
        if (Math.abs(prev.width - width) < 1 && Math.abs(prev.height - height) < 1) {
          return prev
        }
        return { width, height }
      })
    }

    _libraryResizeCallbacks.set(el, callback)
    observer.observe(el)

    const rect = el.getBoundingClientRect()
    setSize({ width: rect.width, height: rect.height })

    return () => {
      _libraryResizeCallbacks.delete(el)
      observer.unobserve(el)
    }
  }, [ref])

  return size
}

// ==================== Intersection Hooks ====================

let _libraryIntersectionObserver: IntersectionObserver | null = null
const _libraryIntersectionCallbacks = new Map<Element, (entry: IntersectionObserverEntry) => void>()

function getLibraryIntersectionObserver(): IntersectionObserver {
  if (!_libraryIntersectionObserver) {
    _libraryIntersectionObserver = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const cb = _libraryIntersectionCallbacks.get(entry.target)
          if (cb)
            cb(entry)
        }
      },
      {
        rootMargin: '200px', // 提前 200px 开始加载
        threshold: 0,
      },
    )
  }
  return _libraryIntersectionObserver
}

/**
 * 资料库视口可见性检测
 * 用于懒加载图片和无限滚动
 */
export function useLibraryInView<T extends Element>(): {
  ref: React.RefObject<T>
  isInView: boolean
} {
  const ref = useRef<T>(null)
  const [isInView, setIsInView] = useState(false)

  useEffect(() => {
    if (!hasFeature(PAGE_ID, Feature.Intersection)) {
      // 功能未启用，默认可见
      setIsInView(true)
      return
    }

    const el = ref.current
    if (!el)
      return

    const observer = getLibraryIntersectionObserver()
    const callback = (entry: IntersectionObserverEntry) => {
      setIsInView(entry.isIntersecting)
    }

    _libraryIntersectionCallbacks.set(el, callback)
    observer.observe(el)

    return () => {
      _libraryIntersectionCallbacks.delete(el)
      observer.unobserve(el)
    }
  }, [])

  return { ref, isInView }
}

/**
 * 资料库懒加载 Hook
 * 只在元素进入视口后加载，且只触发一次
 */
export function useLibraryLazyLoad<T extends Element>(): {
  ref: React.RefObject<T>
  shouldLoad: boolean
} {
  const ref = useRef<T>(null)
  const [shouldLoad, setShouldLoad] = useState(false)

  useEffect(() => {
    if (!hasFeature(PAGE_ID, Feature.Intersection)) {
      setShouldLoad(true)
      return
    }

    const el = ref.current
    if (!el)
      return

    const observer = getLibraryIntersectionObserver()
    const callback = (entry: IntersectionObserverEntry) => {
      if (entry.isIntersecting) {
        setShouldLoad(true)
        // 加载后取消观察
        _libraryIntersectionCallbacks.delete(el)
        observer.unobserve(el)
      }
    }

    _libraryIntersectionCallbacks.set(el, callback)
    observer.observe(el)

    return () => {
      _libraryIntersectionCallbacks.delete(el)
      observer.unobserve(el)
    }
  }, [])

  return { ref, shouldLoad }
}

/**
 * 无限滚动触发器
 * 监听一个哨兵元素，进入视口时加载更多
 *
 * @param onLoadMore - 加载更多回调，可以是异步函数
 * @param hasMore - 是否还有更多数据
 * @param isLoading - 可选，外部传入的加载状态，用于更精确控制
 */
export function useLibraryInfiniteScroll(
  onLoadMore: () => void | Promise<void>,
  hasMore: boolean,
  isLoading?: boolean,
): React.RefObject<HTMLDivElement> {
  const sentinelRef = useRef<HTMLDivElement>(null)
  const loadingRef = useRef(false)

  // 同步外部 isLoading 状态
  useEffect(() => {
    if (isLoading !== undefined) {
      loadingRef.current = isLoading
    }
  }, [isLoading])

  useEffect(() => {
    if (!hasFeature(PAGE_ID, Feature.Intersection) || !hasMore) {
      return
    }

    const el = sentinelRef.current
    if (!el)
      return

    const observer = getLibraryIntersectionObserver()
    const callback = async (entry: IntersectionObserverEntry) => {
      if (entry.isIntersecting && !loadingRef.current && hasMore) {
        loadingRef.current = true
        try {
          await onLoadMore()
        }
        finally {
          // 异步完成后才重置，确保不会重复触发
          loadingRef.current = false
        }
      }
    }

    _libraryIntersectionCallbacks.set(el, callback)
    observer.observe(el)

    return () => {
      _libraryIntersectionCallbacks.delete(el)
      observer.unobserve(el)
    }
  }, [onLoadMore, hasMore])

  return sentinelRef
}

/**
 * 资料库 Intersection 监听（命令式 API）
 * 用于 callback ref 或命令式场景
 *
 * @example
 * ```tsx
 * const { observeLibraryIntersection, unobserveLibraryIntersection } = useLibraryIntersectionObserver();
 * useEffect(() => {
 *   if (target) observeLibraryIntersection(target, (entry) => { ... });
 *   return () => unobserveLibraryIntersection(target);
 * }, [target]);
 * ```
 */
export function useLibraryIntersectionObserver(): {
  observeLibraryIntersection: (el: Element, callback: (entry: IntersectionObserverEntry) => void) => void
  unobserveLibraryIntersection: (el: Element) => void
} {
  const observeLibraryIntersection = useCallback((el: Element, callback: (entry: IntersectionObserverEntry) => void) => {
    if (!hasFeature(PAGE_ID, Feature.Intersection)) {
      // 功能未启用时，模拟一次 isIntersecting
      callback({ isIntersecting: true } as IntersectionObserverEntry)
      return
    }
    const observer = getLibraryIntersectionObserver()
    _libraryIntersectionCallbacks.set(el, callback)
    observer.observe(el)
  }, [])

  const unobserveLibraryIntersection = useCallback((el: Element) => {
    _libraryIntersectionCallbacks.delete(el)
    if (_libraryIntersectionObserver) {
      _libraryIntersectionObserver.unobserve(el)
    }
  }, [])

  return { observeLibraryIntersection, unobserveLibraryIntersection }
}

// ==================== Idle Hooks ====================

/**
 * 资料库空闲预加载
 */
export function useLibraryPrefetch(
  prefetchFn: () => void,
  deps: React.DependencyList = [],
): void {
  useEffect(() => {
    if (!hasFeature(PAGE_ID, Feature.Idle)) {
      return
    }

    const id = requestIdleCallback(() => {
      if (isPageVisible()) {
        prefetchFn()
      }
    }, { timeout: 5000 })

    return () => cancelIdleCallback(id)
  }, deps)
}

// ==================== 清理 ====================

export function cleanupLibrary(): void {
  if (_libraryResizeObserver) {
    _libraryResizeObserver.disconnect()
    _libraryResizeObserver = null
  }
  _libraryResizeCallbacks.clear()

  if (_libraryIntersectionObserver) {
    _libraryIntersectionObserver.disconnect()
    _libraryIntersectionObserver = null
  }
  _libraryIntersectionCallbacks.clear()
}
