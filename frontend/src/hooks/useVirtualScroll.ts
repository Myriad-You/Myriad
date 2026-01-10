/**
 * 虚拟滚动 Hook
 * 用于优化大列表性能,只渲染可见区域的项目
 * 增强版: 支持网格布局、RAF节流、Intersection Observer
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import { rafThrottle } from '../utils/performance'
import { observeIntersection } from './animation'

interface VirtualScrollOptions {
  itemHeight: number // 每个项目的高度
  overscan?: number // 预渲染的额外项目数量
  enabled?: boolean // 是否启用虚拟滚动
}

interface VirtualScrollResult {
  visibleItems: number[] // 可见项目的索引数组
  containerHeight: number // 容器总高度
  offsetY: number // 偏移量
}

export function useVirtualScroll(
  totalItems: number,
  options: VirtualScrollOptions,
): VirtualScrollResult {
  const { itemHeight, overscan = 5, enabled = true } = options

  const [scrollTop, setScrollTop] = useState(0)
  const [containerHeight, setContainerHeight] = useState(0)

  // 监听滚动 - 使用RAF节流优化
  useEffect(() => {
    if (!enabled)
      return

    const handleScroll = rafThrottle(() => {
      setScrollTop(window.scrollY)
    })

    const handleResize = rafThrottle(() => {
      setContainerHeight(window.innerHeight)
    })

    handleResize()
    window.addEventListener('scroll', handleScroll, { passive: true })
    window.addEventListener('resize', handleResize)

    return () => {
      window.removeEventListener('scroll', handleScroll)
      window.removeEventListener('resize', handleResize)
    }
  }, [enabled])

  // 计算可见项目
  const visibleItems = (() => {
    if (!enabled || totalItems === 0) {
      return Array.from({ length: totalItems }, (_, i) => i)
    }

    const startIndex = Math.max(0, Math.floor(scrollTop / itemHeight) - overscan)
    const endIndex = Math.min(
      totalItems - 1,
      Math.ceil((scrollTop + containerHeight) / itemHeight) + overscan,
    )

    return Array.from(
      { length: endIndex - startIndex + 1 },
      (_, i) => startIndex + i,
    )
  })()

  return {
    visibleItems,
    containerHeight: totalItems * itemHeight,
    offsetY: visibleItems.length > 0 ? visibleItems[0] * itemHeight : 0,
  }
}

/**
 * 分页加载 Hook
 * 用于逐步加载数据，避免一次性加载过多
 */

interface PagedLoadOptions {
  pageSize: number // 每页大小
  initialPages?: number // 初始加载页数
  threshold?: number // 触发加载的距离阈值（像素）
}

interface PagedLoadResult<T> {
  items: T[] // 当前已加载的项目
  loadMore: () => void // 加载更多函数
  hasMore: boolean // 是否还有更多数据
  loading: boolean // 是否正在加载
  reset: () => void // 重置状态
}

export function usePagedLoad<T>(
  allItems: T[],
  options: PagedLoadOptions,
): PagedLoadResult<T> {
  const { pageSize, initialPages = 2, threshold = 500 } = options

  const [currentPage, setCurrentPage] = useState(initialPages)
  const [loading, setLoading] = useState(false)
  const loadingRef = useRef(false)

  const items = allItems.slice(0, currentPage * pageSize)
  const hasMore = items.length < allItems.length

  // 加载更多
  const loadMore = useCallback(() => {
    if (loadingRef.current || !hasMore)
      return

    loadingRef.current = true
    setLoading(true)

    // 模拟异步加载延迟
    setTimeout(() => {
      setCurrentPage(prev => prev + 1)
      setLoading(false)
      loadingRef.current = false
    }, 300)
  }, [hasMore])

  // 监听滚动触发加载 - 使用RAF节流优化
  useEffect(() => {
    const handleScroll = rafThrottle(() => {
      if (!hasMore || loadingRef.current)
        return

      const scrollHeight = document.documentElement.scrollHeight
      const scrollTop = window.scrollY
      const clientHeight = window.innerHeight

      if (scrollHeight - scrollTop - clientHeight < threshold) {
        loadMore()
      }
    })

    window.addEventListener('scroll', handleScroll, { passive: true })
    return () => window.removeEventListener('scroll', handleScroll)
  }, [hasMore, loadMore, threshold])

  // 重置
  const reset = useCallback(() => {
    setCurrentPage(initialPages)
    setLoading(false)
    loadingRef.current = false
  }, [initialPages])

  // 当 allItems 改变时重置
  useEffect(() => {
    reset()
  }, [allItems.length, reset])

  return {
    items,
    loadMore,
    hasMore,
    loading,
    reset,
  }
}

/**
 * 虚拟网格 Hook
 * 用于优化瀑布流/网格布局性能
 */

interface VirtualGridOptions {
  columnCount: number // 列数
  rowHeight: number // 行高
  overscan?: number // 预渲染行数
  enabled?: boolean // 是否启用
}

interface VirtualGridResult {
  visibleItems: number[] // 可见项目索引
  totalHeight: number // 总高度
  getItemStyle: (index: number) => React.CSSProperties // 获取项目样式
}

export function useVirtualGrid(
  totalItems: number,
  options: VirtualGridOptions,
): VirtualGridResult {
  const { columnCount, rowHeight, overscan = 2, enabled = true } = options

  const [scrollTop, setScrollTop] = useState(0)
  const [containerHeight, setContainerHeight] = useState(0)

  const totalRows = Math.ceil(totalItems / columnCount)

  // 监听滚动
  useEffect(() => {
    if (!enabled)
      return

    const handleScroll = rafThrottle(() => {
      setScrollTop(window.scrollY)
    })

    const handleResize = rafThrottle(() => {
      setContainerHeight(window.innerHeight)
    })

    handleResize()
    window.addEventListener('scroll', handleScroll, { passive: true })
    window.addEventListener('resize', handleResize)

    return () => {
      window.removeEventListener('scroll', handleScroll)
      window.removeEventListener('resize', handleResize)
    }
  }, [enabled])

  // 计算可见项目
  const visibleItems = (() => {
    if (!enabled || totalItems === 0) {
      return Array.from({ length: totalItems }, (_, i) => i)
    }

    const startRow = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan)
    const endRow = Math.min(
      totalRows - 1,
      Math.ceil((scrollTop + containerHeight) / rowHeight) + overscan,
    )

    const items: number[] = []
    for (let row = startRow; row <= endRow; row++) {
      for (let col = 0; col < columnCount; col++) {
        const index = row * columnCount + col
        if (index < totalItems) {
          items.push(index)
        }
      }
    }

    return items
  })()

  // 获取项目样式
  const getItemStyle = useCallback((index: number): React.CSSProperties => {
    const row = Math.floor(index / columnCount)
    const col = index % columnCount

    return {
      position: 'absolute',
      top: `${row * rowHeight}px`,
      left: `${(col / columnCount) * 100}%`,
      width: `${(1 / columnCount) * 100}%`,
      height: `${rowHeight}px`,
    }
  }, [columnCount, rowHeight])

  return {
    visibleItems,
    totalHeight: totalRows * rowHeight,
    getItemStyle,
  }
}

/**
 * Intersection Observer Hook
 * 用于懒加载和可见性检测
 * 使用共享的 IntersectionObserver（通过 AnimationCoordinator）
 */

interface IntersectionObserverOptions {
  threshold?: number
  rootMargin?: string
  root?: Element | null
  once?: boolean // 是否只触发一次
}

export function useIntersectionObserver(
  elementRef: React.RefObject<Element>,
  callback: (isIntersecting: boolean, entry: IntersectionObserverEntry) => void,
  options?: IntersectionObserverOptions,
): void {
  const { once = false, threshold = 0, rootMargin = '0px' } = options || {}
  const hasTriggered = useRef(false)
  const unobserveRef = useRef<(() => void) | null>(null)

  useEffect(() => {
    const element = elementRef.current
    if (!element || typeof IntersectionObserver === 'undefined')
      return

    // 清理旧观察
    if (unobserveRef.current) {
      unobserveRef.current()
      unobserveRef.current = null
    }

    // 使用共享的 IntersectionObserver
    unobserveRef.current = observeIntersection(
      element,
      (entry) => {
        if (once && hasTriggered.current)
          return

        callback(entry.isIntersecting, entry)

        if (entry.isIntersecting && once) {
          hasTriggered.current = true
          if (unobserveRef.current) {
            unobserveRef.current()
            unobserveRef.current = null
          }
        }
      },
      { threshold, rootMargin },
    )

    return () => {
      if (unobserveRef.current) {
        unobserveRef.current()
        unobserveRef.current = null
      }
    }
  }, [elementRef, callback, once, threshold, rootMargin])
}

/**
 * 懒加载图片 Hook
 * 结合Intersection Observer实现图片懒加载
 */

export function useLazyImage(
  src: string,
  placeholder?: string,
): [string, boolean, React.RefObject<HTMLImageElement>] {
  const [imageSrc, setImageSrc] = useState(placeholder || '')
  const [isLoaded, setIsLoaded] = useState(false)
  const imgRef = useRef<HTMLImageElement>(null)

  useIntersectionObserver(
    imgRef,
    (isIntersecting) => {
      if (isIntersecting && !isLoaded) {
        const img = new Image()
        img.src = src
        img.onload = () => {
          setImageSrc(src)
          setIsLoaded(true)
        }
      }
    },
    { once: true, rootMargin: '50px' },
  )

  return [imageSrc, isLoaded, imgRef]
}
