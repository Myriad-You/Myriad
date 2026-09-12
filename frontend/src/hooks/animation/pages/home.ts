import { useCallback, useEffect, useRef, useState } from 'react'
import {
  getPageIntervalManager,
  getPageResizeManager,
  isPageVisible,
  onVisibility,
  registerPageCleanup,
} from '../core'
import { Feature, hasFeature } from '../pageFeatures'

const PAGE_ID = 'home'

/** startPage('home') 由 useRouteScheduler 统一调用。 */
export function useHomeScheduler(): void {
  useEffect(() => {
    return () => cleanupHome()
  }, [])
}

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

function getIntervalManager() {
  return getPageIntervalManager(PAGE_ID)
}

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
    if (!enabled) return

    if (visible) {
      intervalRef.current = setInterval(() => {
        savedCallback.current()
      }, delay)
      getIntervalManager().add(intervalRef.current)
    }

    return () => {
      if (intervalRef.current !== null) {
        getIntervalManager().remove(intervalRef.current)
        intervalRef.current = null
      }
    }
  }, [delay, visible, enabled])
}

function getResizeManager() {
  return getPageResizeManager(PAGE_ID)
}

export function useHomeResize<T extends Element>(
  ref: React.RefObject<T>,
): { width: number; height: number } {
  const [size, setSize] = useState({ width: 0, height: 0 })

  useEffect(() => {
    if (!hasFeature(PAGE_ID, Feature.Resize)) {
      console.warn('[Home] Resize feature not enabled')
      return
    }

    const el = ref.current
    if (!el) return

    const observer = getResizeManager()
    const callback = (entry: ResizeObserverEntry) => {
      const { width, height } = entry.contentRect
      setSize((prev) => {
        if (
          Math.abs(prev.width - width) < 1 &&
          Math.abs(prev.height - height) < 1
        ) {
          return prev
        }
        return { width, height }
      })
    }

    observer.observe(el, callback)

    const rect = el.getBoundingClientRect()
    setSize({ width: rect.width, height: rect.height })

    return () => {
      observer.unobserve(el)
    }
  }, [ref])

  return size
}

export function useHomeResizeObserver(): {
  observeHomeResize: (
    el: Element,
    callback: (entry: ResizeObserverEntry) => void,
  ) => void
  unobserveHomeResize: (el: Element) => void
} {
  const observeHomeResize = useCallback(
    (el: Element, callback: (entry: ResizeObserverEntry) => void) => {
      if (!hasFeature(PAGE_ID, Feature.Resize)) {
        return
      }
      const manager = getResizeManager()
      manager.observe(el, callback)

      const rect = el.getBoundingClientRect()
      callback({ contentRect: rect } as ResizeObserverEntry)
    },
    [],
  )

  const unobserveHomeResize = useCallback((el: Element) => {
    getResizeManager().unobserve(el)
  }, [])

  return { observeHomeResize, unobserveHomeResize }
}

export function useHomeRaf<T extends (...args: any[]) => void>(
  callback: T,
  deps: React.DependencyList = [],
): T {
  const rafId = useRef<number | null>(null)
  const lastArgs = useRef<any[]>([])

  const throttled = useCallback((...args: any[]) => {
    if (!hasFeature(PAGE_ID, Feature.RAF)) {
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

export function useHomeIdle(
  callback: () => void,
  deps: React.DependencyList = [],
): void {
  useEffect(() => {
    if (!hasFeature(PAGE_ID, Feature.Idle)) {
      return
    }

    const id = requestIdleCallback(
      () => {
        if (isPageVisible()) {
          callback()
        }
      },
      { timeout: 3000 },
    )

    return () => cancelIdleCallback(id)
  }, deps)
}

export function cleanupHome(): void {
  getPageIntervalManager(PAGE_ID).cleanup()
  getPageResizeManager(PAGE_ID).cleanup()
}

registerPageCleanup(PAGE_ID, cleanupHome)
