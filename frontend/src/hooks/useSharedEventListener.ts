/**
 * 共享事件监听器 Hook
 *
 * 多个组件监听同一事件时，合并为一个监听器，减少浏览器开销
 *
 * @module useSharedEventListener
 * @version 1.2
 */

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from 'react'
import { rafThrottle } from '../utils/performance'
import { VIEWPORT_MQ } from '../utils/viewportBands'

import { isPageVisible, onVisibility } from './animation/core'

type EventCallback = (event: Event) => void

interface ListenerEntry {
  callback: EventCallback
  priority: number
}

/** 事件监听器管理器 */
class SharedEventManager {
  private listeners = new Map<string, Set<ListenerEntry>>()
  private nativeListeners = new Map<string, EventCallback>()
  private throttledCallbacks = new Map<string, EventCallback>()
  // 缓存排序后的监听器数组，避免每次事件触发时都排序
  private sortedListenersCache = new Map<string, ListenerEntry[]>()
  private listenersDirty = new Map<string, boolean>()

  /**
   * 添加监听器
   * @param eventType 事件类型
   * @param callback 回调函数
   * @param options 选项
   */
  add(
    eventType: string,
    callback: EventCallback,
    options: { priority?: number; throttle?: boolean } = {},
  ): () => void {
    const { priority = 0, throttle = false } = options

    if (!this.listeners.has(eventType)) {
      this.listeners.set(eventType, new Set())
      this.setupNativeListener(eventType, throttle)
    }

    const entry: ListenerEntry = { callback, priority }
    this.listeners.get(eventType)!.add(entry)
    // 标记缓存为脏，下次事件触发时重新排序
    this.listenersDirty.set(eventType, true)

    // 返回移除函数
    return () => {
      const set = this.listeners.get(eventType)
      if (set) {
        set.delete(entry)
        // 标记缓存为脏
        this.listenersDirty.set(eventType, true)
        if (set.size === 0) {
          this.removeNativeListener(eventType)
          this.listeners.delete(eventType)
          this.sortedListenersCache.delete(eventType)
          this.listenersDirty.delete(eventType)
        }
      }
    }
  }

  private setupNativeListener(eventType: string, throttle: boolean) {
    const handler: EventCallback = (event) => {
      const entries = this.listeners.get(eventType)
      if (!entries || entries.size === 0) return

      // 使用缓存的排序结果，只有在监听器变化时才重新排序
      let sorted = this.sortedListenersCache.get(eventType)
      if (!sorted || this.listenersDirty.get(eventType)) {
        sorted = Array.from(entries).sort((a, b) => b.priority - a.priority)
        this.sortedListenersCache.set(eventType, sorted)
        this.listenersDirty.set(eventType, false)
      }

      for (let i = 0; i < sorted.length; i++) {
        try {
          sorted[i].callback(event)
        } catch (e) {
          console.error(`Error in ${eventType} listener:`, e)
        }
      }
    }

    const finalHandler = throttle ? rafThrottle(handler) : handler

    this.nativeListeners.set(eventType, finalHandler)
    if (throttle) {
      this.throttledCallbacks.set(eventType, handler)
    }

    window.addEventListener(eventType, finalHandler, { passive: true })
  }

  private removeNativeListener(eventType: string) {
    const handler = this.nativeListeners.get(eventType)
    if (handler) {
      window.removeEventListener(eventType, handler)
      this.nativeListeners.delete(eventType)
      this.throttledCallbacks.delete(eventType)
    }
  }

  /** 获取当前监听状态（调试用） */
  getStats() {
    const stats: Record<string, number> = {}
    for (const [type, set] of this.listeners) {
      stats[type] = set.size
    }
    return stats
  }

  /** 清理所有监听器 */
  clear() {
    for (const eventType of this.listeners.keys()) {
      this.removeNativeListener(eventType)
    }
    this.listeners.clear()
  }
}

/** 全局共享事件管理器 */
export const sharedEventManager = new SharedEventManager()

/**
 * 使用共享的窗口事件监听
 *
 * @param eventType 事件类型（如 'resize', 'scroll'）
 * @param callback 回调函数
 * @param options 选项
 *
 * @example
 * ```tsx
 * // 多个组件都可以这样使用，内部只会有一个 resize 监听器
 * useSharedEventListener('resize', () => {
 *   setWidth(window.innerWidth);
 * }, { throttle: true });
 * ```
 */
export function useSharedEventListener(
  eventType: string,
  callback: EventCallback,
  options: { priority?: number; throttle?: boolean; enabled?: boolean } = {},
): void {
  const { priority = 0, throttle = true, enabled = true } = options

  const callbackRef = useRef(callback)
  callbackRef.current = callback

  const stableCallback = useCallback((event: Event) => {
    callbackRef.current(event)
  }, [])

  useEffect(() => {
    if (!enabled) return

    const remove = sharedEventManager.add(eventType, stableCallback, {
      priority,
      throttle,
    })

    return remove
  }, [eventType, stableCallback, priority, throttle, enabled])
}

/**
 * 共享的 resize 事件 Hook
 * 使用 RAF 节流，多个组件共享一个监听器
 *
 * @param callback 回调函数
 * @param options 选项（支持防抖延迟）
 */
export function useSharedResize(
  callback: () => void,
  options: { priority?: number; enabled?: boolean; debounce?: number } = {},
): void {
  const { debounce: debounceMs, ...restOptions } = options
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const callbackRef = useRef(callback)
  callbackRef.current = callback

  const handler = useCallback(() => {
    if (debounceMs && debounceMs > 0) {
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current)
      }
      timeoutRef.current = setTimeout(() => {
        callbackRef.current()
      }, debounceMs)
    } else {
      callbackRef.current()
    }
  }, [debounceMs])

  // 清理防抖定时器
  useEffect(() => {
    return () => {
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current)
      }
    }
  }, [])

  useSharedEventListener('resize', handler, {
    ...restOptions,
    throttle: !debounceMs, // 如果使用防抖则不使用节流
  })
}

/**
 * 共享的 scroll 事件 Hook
 * 使用 RAF 节流，多个组件共享一个监听器
 */
export function useSharedScroll(
  callback: (event: Event) => void,
  options: { priority?: number; enabled?: boolean; throttleMs?: number } = {},
): void {
  const { throttleMs, ...restOptions } = options
  const callbackRef = useRef(callback)
  callbackRef.current = callback

  const lastCallTime = useRef(0)

  const handler = useCallback(
    (event: Event) => {
      if (throttleMs && throttleMs > 0) {
        const now = performance.now()
        if (now - lastCallTime.current >= throttleMs) {
          lastCallTime.current = now
          callbackRef.current(event)
        }
      } else {
        callbackRef.current(event)
      }
    },
    [throttleMs],
  )

  useSharedEventListener('scroll', handler, {
    ...restOptions,
    throttle: true, // RAF 节流作为基础
  })
}

/**
 * 窗口尺寸 Hook - 使用共享监听器
 *
 * @example
 * ```tsx
 * const { width, height } = useWindowSize();
 * ```
 */
export function useWindowSize(): { width: number; height: number } {
  const [size, setSize] = useState({
    width: typeof window !== 'undefined' ? window.innerWidth : 0,
    height: typeof window !== 'undefined' ? window.innerHeight : 0,
  })

  useSharedResize(() => {
    setSize({
      width: window.innerWidth,
      height: window.innerHeight,
    })
  })

  return size
}

/**
 * 防抖的窗口尺寸 Hook
 * 适用于需要在 resize 结束后才执行操作的场景
 * 使用共享监听器，减少重复注册
 *
 * @param delay 防抖延迟（毫秒）
 */
export function useDebouncedWindowSize(delay = 150): {
  width: number
  height: number
} {
  const [size, setSize] = useState({
    width: typeof window !== 'undefined' ? window.innerWidth : 0,
    height: typeof window !== 'undefined' ? window.innerHeight : 0,
  })

  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const debouncedHandler = useCallback(() => {
    if (timeoutRef.current) {
      clearTimeout(timeoutRef.current)
    }
    timeoutRef.current = setTimeout(() => {
      setSize({
        width: window.innerWidth,
        height: window.innerHeight,
      })
    }, delay)
  }, [delay])

  // 使用共享的 resize 监听器
  useSharedResize(debouncedHandler)

  // 清理定时器
  useEffect(() => {
    return () => {
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current)
      }
    }
  }, [])

  return size
}

/**
 * 每个 query 字符串全局只建一个 MediaQueryList + 一个原生 change 监听。
 *
 * 此前每个调用点各自 `matchMedia()` 并挂监听：首页十几个小组件
 * （useWidgetSize → useViewportBand 两条 + useAnimationLevel 一条）
 * 会堆出几十个 MQL 对象。distinct query 只有个位数，注册表常驻即可，
 * 不做引用计数摘除——摘了就得在重订阅时补一次读值与通知，反而易错。
 */
interface SharedMediaQueryEntry {
  mql: MediaQueryList
  matches: boolean
  listeners: Set<() => void>
}

const _mediaQueryRegistry = new Map<string, SharedMediaQueryEntry>()

function getSharedMediaQuery(query: string): SharedMediaQueryEntry | null {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
    return null
  }

  let entry = _mediaQueryRegistry.get(query)
  if (!entry) {
    const mql = window.matchMedia(query)
    const created: SharedMediaQueryEntry = {
      mql,
      matches: mql.matches,
      listeners: new Set(),
    }
    mql.addEventListener('change', (event: MediaQueryListEvent) => {
      created.matches = event.matches
      for (const listener of created.listeners) listener()
    })
    _mediaQueryRegistry.set(query, created)
    entry = created
  }
  return entry
}

/** SSR 快照必须是稳定引用，否则 useSyncExternalStore 会警告 */
function mediaQueryServerSnapshot(): boolean {
  return false
}

/**
 * 媒体查询 Hook - 响应式断点检测
 *
 * 同一 query 的所有调用点共享一个 MediaQueryList（见 {@link getSharedMediaQuery}）。
 *
 * @param query 媒体查询字符串
 * @returns 是否匹配
 *
 * @example
 * ```tsx
 * const isMobile = useMediaQuery('(max-width: 767px)');
 * const prefersDark = useMediaQuery('(prefers-color-scheme: dark)');
 * ```
 */
export function useMediaQuery(query: string): boolean {
  const subscribe = useCallback(
    (onStoreChange: () => void) => {
      const entry = getSharedMediaQuery(query)
      if (!entry) return () => {}
      entry.listeners.add(onStoreChange)
      return () => {
        entry.listeners.delete(onStoreChange)
      }
    },
    [query],
  )

  const getSnapshot = useCallback(() => {
    const entry = getSharedMediaQuery(query)
    return entry ? entry.matches : false
  }, [query])

  return useSyncExternalStore(subscribe, getSnapshot, mediaQueryServerSnapshot)
}

/**
 * 预定义的响应式断点 Hook
 * 阈值与 utils/viewportBands 一致（phone≤767 / tablet … / desktop≥1078）
 */
export function useBreakpoints() {
  const isMobile = useMediaQuery(VIEWPORT_MQ.phone)
  const isTablet = useMediaQuery(VIEWPORT_MQ.tablet)
  const isDesktop = useMediaQuery(VIEWPORT_MQ.desktop)
  const isLargeDesktop = useMediaQuery('(min-width: 1280px)')

  return useMemo(
    () => ({
      isMobile,
      isTablet,
      isDesktop,
      isLargeDesktop,
      // 便捷属性
      isTouchDevice: isMobile || isTablet,
    }),
    [isMobile, isTablet, isDesktop, isLargeDesktop],
  )
}

/**
 * 页面可见性 Hook
 * 用于在页面不可见时暂停动画或网络请求
 *
 * 🔧 使用统一的 coordinator 可见性管理，避免重复的事件监听器
 */
export function usePageVisibility(): boolean {
  const [isVisible, setIsVisible] = useState(() => isPageVisible())

  useEffect(() => {
    return onVisibility(setIsVisible)
  }, [])

  return isVisible
}
