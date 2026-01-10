/**
 * DOM 批量操作 Hook
 *
 * 避免强制同步布局（Forced Synchronous Layout）
 * 将读写操作分离到不同的帧阶段
 *
 * @module useBatchedDom
 */

import { useCallback, useEffect, useRef } from 'react'

interface BatchedDomOperations {
  /** 批量读取 DOM 属性 */
  read: (callback: () => void) => void
  /** 批量写入 DOM 样式 */
  write: (callback: () => void) => void
  /** 测量元素尺寸（读取操作） */
  measure: <T>(callback: () => T) => Promise<T>
  /** 修改元素样式（写入操作） */
  mutate: (callback: () => void) => Promise<void>
  /** 同时读写（先读后写） */
  readThenWrite: (read: () => void, write: () => void) => void
}

// 全局批处理队列
let readQueue: (() => void)[] = []
let writeQueue: (() => void)[] = []
let scheduled = false

function scheduleFlush(): void {
  if (scheduled)
    return
  scheduled = true

  requestAnimationFrame(() => {
    scheduled = false

    // 先执行所有读取
    const reads = readQueue
    readQueue = []
    for (const read of reads) {
      try {
        read()
      }
      catch (e) {
        console.error('Batched DOM read error:', e)
      }
    }

    // 再执行所有写入
    const writes = writeQueue
    writeQueue = []
    for (const write of writes) {
      try {
        write()
      }
      catch (e) {
        console.error('Batched DOM write error:', e)
      }
    }
  })
}

/**
 * 使用批量 DOM 操作
 *
 * @example
 * ```tsx
 * function MyComponent() {
 *   const { read, write, measure, mutate } = useBatchedDom();
 *   const ref = useRef<HTMLDivElement>(null);
 *
 *   const handleClick = async () => {
 *     // 测量元素
 *     const width = await measure(() => ref.current?.offsetWidth || 0);
 *
 *     // 修改样式
 *     await mutate(() => {
 *       if (ref.current) {
 *         ref.current.style.width = (width * 2) + 'px';
 *       }
 *     });
 *   };
 *
 *   return <div ref={ref} onClick={handleClick}>Click me</div>;
 * }
 * ```
 */
export function useBatchedDom(): BatchedDomOperations {
  const pendingReads = useRef<(() => void)[]>([])
  const pendingWrites = useRef<(() => void)[]>([])

  // 组件卸载时清理
  useEffect(() => {
    return () => {
      pendingReads.current = []
      pendingWrites.current = []
    }
  }, [])

  const read = useCallback((callback: () => void) => {
    readQueue.push(callback)
    pendingReads.current.push(callback)
    scheduleFlush()
  }, [])

  const write = useCallback((callback: () => void) => {
    writeQueue.push(callback)
    pendingWrites.current.push(callback)
    scheduleFlush()
  }, [])

  const measure = useCallback(<T>(callback: () => T): Promise<T> => {
    return new Promise((resolve) => {
      read(() => {
        resolve(callback())
      })
    })
  }, [read])

  const mutate = useCallback((callback: () => void): Promise<void> => {
    return new Promise((resolve) => {
      write(() => {
        callback()
        resolve()
      })
    })
  }, [write])

  const readThenWrite = useCallback((readFn: () => void, writeFn: () => void) => {
    read(readFn)
    write(writeFn)
  }, [read, write])

  return { read, write, measure, mutate, readThenWrite }
}

/**
 * 安全读取 DOM 属性
 * 不会触发强制重排
 */
export function safeRead<T>(callback: () => T): Promise<T> {
  return new Promise((resolve) => {
    readQueue.push(() => {
      resolve(callback())
    })
    scheduleFlush()
  })
}

/**
 * 安全写入 DOM 样式
 * 批量执行以减少重绘
 */
export function safeWrite(callback: () => void): Promise<void> {
  return new Promise((resolve) => {
    writeQueue.push(() => {
      callback()
      resolve()
    })
    scheduleFlush()
  })
}

/**
 * 同步测量元素（使用缓存）
 * 避免在动画循环中频繁调用 getBoundingClientRect
 */
export class ElementMeasureCache {
  private cache = new WeakMap<Element, { rect: DOMRect, timestamp: number }>()
  private cacheTimeout = 100 // 缓存有效期 100ms

  /**
   * 获取元素尺寸（带缓存）
   */
  getRect(element: Element): DOMRect {
    const cached = this.cache.get(element)
    const now = performance.now()

    if (cached && now - cached.timestamp < this.cacheTimeout) {
      return cached.rect
    }

    const rect = element.getBoundingClientRect()
    this.cache.set(element, { rect, timestamp: now })
    return rect
  }

  /**
   * 使缓存失效
   */
  invalidate(element: Element): void {
    this.cache.delete(element)
  }

  /**
   * 清空所有缓存
   */
  clear(): void {
    this.cache = new WeakMap()
  }

  /**
   * 设置缓存超时时间
   */
  setTimeout(ms: number): void {
    this.cacheTimeout = ms
  }
}

/** 全局测量缓存实例 */
export const measureCache = new ElementMeasureCache()

/**
 * 使用测量缓存的 Hook
 */
export function useMeasureCache() {
  const cacheRef = useRef(new ElementMeasureCache())

  useEffect(() => {
    return () => {
      cacheRef.current.clear()
    }
  }, [])

  return cacheRef.current
}

/**
 * 避免布局抖动的样式更新器
 *
 * @example
 * ```tsx
 * const updater = useStyleUpdater(elementRef);
 *
 * // 批量更新样式，只触发一次重绘
 * updater.set({
 *   transform: 'translateX(100px)',
 *   opacity: '0.5',
 * });
 * ```
 */
export function useStyleUpdater(ref: React.RefObject<HTMLElement>) {
  const pendingStyles = useRef<Map<string, string>>(new Map())
  const rafId = useRef<number | null>(null)

  useEffect(() => {
    return () => {
      if (rafId.current) {
        cancelAnimationFrame(rafId.current)
      }
    }
  }, [])

  const scheduleUpdate = useCallback(() => {
    if (rafId.current)
      return

    rafId.current = requestAnimationFrame(() => {
      rafId.current = null

      if (!ref.current || pendingStyles.current.size === 0)
        return

      const el = ref.current
      const styles = pendingStyles.current
      pendingStyles.current = new Map()

      // 批量应用所有样式
      for (const [prop, value] of styles) {
        (el.style as any)[prop] = value
      }
    })
  }, [ref])

  const set = useCallback((styles: Record<string, string>) => {
    for (const [prop, value] of Object.entries(styles)) {
      pendingStyles.current.set(prop, value)
    }
    scheduleUpdate()
  }, [scheduleUpdate])

  const setImmediate = useCallback((styles: Record<string, string>) => {
    if (!ref.current)
      return

    for (const [prop, value] of Object.entries(styles)) {
      (ref.current.style as any)[prop] = value
    }
  }, [ref])

  return { set, setImmediate }
}

/**
 * 防止布局抖动的 class 操作
 */
export function batchClassChanges(
  operations: Array<{ element: Element, add?: string[], remove?: string[] }>,
): void {
  requestAnimationFrame(() => {
    for (const op of operations) {
      if (op.add) {
        op.element.classList.add(...op.add)
      }
      if (op.remove) {
        op.element.classList.remove(...op.remove)
      }
    }
  })
}
