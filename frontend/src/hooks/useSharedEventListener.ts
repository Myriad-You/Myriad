import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from 'react'
import { rafThrottle } from '../utils/performance'
import { VIEWPORT_DESKTOP_MIN, VIEWPORT_MQ } from '../utils/viewportBands'

import { isPageVisible, onVisibility } from './animation/core'

type EventCallback = (event: Event) => void

interface ListenerEntry {
  callback: EventCallback
  priority: number
}

class SharedEventManager {
  private listeners = new Map<string, Set<ListenerEntry>>()
  private nativeListeners = new Map<string, EventCallback>()
  private throttledCallbacks = new Map<string, () => void>()

  private sortedListenersCache = new Map<string, ListenerEntry[]>()
  private listenersDirty = new Map<string, boolean>()

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

    this.listenersDirty.set(eventType, true)

    return () => {
      const set = this.listeners.get(eventType)
      if (set) {
        set.delete(entry)

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

      let sorted = this.sortedListenersCache.get(eventType)
      if (!sorted || this.listenersDirty.get(eventType)) {
        sorted = Iterator.from(entries)
          .toArray()
          .toSorted((a, b) => b.priority - a.priority)
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

    const throttled = throttle ? rafThrottle(handler) : null
    const finalHandler = throttled ?? handler

    this.nativeListeners.set(eventType, finalHandler)
    if (throttled) {
      this.throttledCallbacks.set(eventType, throttled.cancel)
    }

    window.addEventListener(eventType, finalHandler, { passive: true })
  }

  private removeNativeListener(eventType: string) {
    const handler = this.nativeListeners.get(eventType)
    if (handler) {
      window.removeEventListener(eventType, handler)
      this.nativeListeners.delete(eventType)
      this.throttledCallbacks.get(eventType)?.()
      this.throttledCallbacks.delete(eventType)
    }
  }

  getStats() {
    const stats: Record<string, number> = {}
    for (const [type, set] of this.listeners) {
      stats[type] = set.size
    }
    return stats
  }

  clear() {
    for (const eventType of this.listeners.keys()) {
      this.removeNativeListener(eventType)
    }
    this.listeners.clear()
    this.sortedListenersCache.clear()
    this.listenersDirty.clear()
  }
}

export const sharedEventManager = new SharedEventManager()

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

/** enabled 为 false 时不订阅 resize。 */
export function useDebouncedWindowSize(
  delay = 150,
  enabled = true,
): {
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

  useSharedResize(debouncedHandler, { enabled })

  useEffect(() => {
    return () => {
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current)
      }
    }
  }, [])

  return size
}

/** 每个 query 全局只建一个 MediaQueryList；常驻，不做引用计数摘除。 */
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

/** SSR 快照必须是稳定引用，否则 useSyncExternalStore 会警告。 */
function mediaQueryServerSnapshot(): boolean {
  return false
}

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
      isTouchDevice: isMobile || isTablet,
    }),
    [isMobile, isTablet, isDesktop, isLargeDesktop],
  )
}

function readDesktopLayoutBand(): boolean {
  if (typeof window === 'undefined') return true
  return window.innerWidth >= VIEWPORT_DESKTOP_MIN
}

/** 首屏读 innerWidth，避免 server snapshot 的 false 闪成 standard 布局。 */
export function useDesktopLayoutBand(): boolean {
  const [isDesktop, setIsDesktop] = useState(readDesktopLayoutBand)
  useLayoutEffect(() => {
    const mq = window.matchMedia(VIEWPORT_MQ.desktop)
    const sync = () => setIsDesktop(mq.matches)
    sync()
    mq.addEventListener('change', sync)
    return () => mq.removeEventListener('change', sync)
  }, [])
  return isDesktop
}

/** 走 coordinator 统一可见性，避免再挂一份 visibilitychange。 */
export function usePageVisibility(): boolean {
  const [isVisible, setIsVisible] = useState(() => isPageVisible())

  useEffect(() => {
    return onVisibility(setIsVisible)
  }, [])

  return isVisible
}
