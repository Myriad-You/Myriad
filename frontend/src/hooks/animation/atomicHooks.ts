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

export function usePageVisible(): boolean {
  const [visible, setVisible] = useState(isPageVisible)

  useEffect(() => {
    return onVisibility(setVisible)
  }, [])

  return visible
}

export function useVisibilityInterval(
  callback: () => void,
  options: { delay: number; enabled?: boolean; immediate?: boolean },
): void {
  const { delay, enabled = true, immediate = false } = options
  const callbackRef = useRef(callback)
  const timeoutRef = useRef<number | null>(null)

  useEffect(() => {
    callbackRef.current = callback
  }, [callback])

  useEffect(() => {
    if (!enabled) return

    let cancelled = false

    const tick = () => {
      if (cancelled || !isPageVisible()) return
      callbackRef.current()
      timeoutRef.current = window.setTimeout(tick, delay)
    }

    if (immediate && isPageVisible()) {
      callbackRef.current()
    }

    timeoutRef.current = window.setTimeout(tick, delay)

    const unsub = onVisibility((vis) => {
      if (vis && timeoutRef.current === null && !cancelled) {
        timeoutRef.current = window.setTimeout(tick, delay)
      } else if (!vis && timeoutRef.current !== null) {
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

export function useElementSize<T extends Element>(): {
  ref: React.RefCallback<T>
  width: number
  height: number
} {
  const [size, setSize] = useState({ width: 0, height: 0 })
  const unobserveRef = useRef<(() => void) | null>(null)

  const ref = useCallback((element: T | null) => {
    if (unobserveRef.current) {
      unobserveRef.current()
      unobserveRef.current = null
    }

    if (element) {
      unobserveRef.current = observeResize(element, (entry) => {
        const { width, height } = entry.contentRect
        setSize((prev) => {
          if (prev.width === width && prev.height === height) return prev
          return { width, height }
        })
      })
    }
  }, [])

  useEffect(() => {
    return () => {
      if (unobserveRef.current) {
        unobserveRef.current()
      }
    }
  }, [])

  return { ref, ...size }
}

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

  const ref = useCallback(
    (element: T | null) => {
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
    },
    [threshold, rootMargin, once],
  )

  useEffect(() => {
    return () => {
      if (unobserveRef.current) {
        unobserveRef.current()
      }
    }
  }, [])

  return { ref, isVisible }
}

export function useLazyLoad<T extends Element>(
  rootMargin = '200px',
): {
  ref: React.RefCallback<T>
  shouldLoad: boolean
} {
  const { ref, isVisible } = useInView<T>({
    rootMargin,
    once: true,
  })

  return { ref, shouldLoad: isVisible }
}

export function useIdleEffect(
  callback: () => void,
  deps: React.DependencyList,
  options?: { priority?: 'low' | 'normal' | 'high' },
): void {
  const callbackRef = useRef(callback)
  callbackRef.current = callback

  useEffect(() => {
    const id = `idle-${now()}-${Math.random().toString(36).slice(2, 9)}`
    const cancel = scheduleIdle(
      id,
      () => callbackRef.current(),
      options?.priority,
    )
    return cancel
  }, deps)
}

export function useBatchedDom(): {
  measureElement: (callback: () => void) => void
  updateElement: (callback: () => void) => void
} {
  return {
    measureElement: batchRead,
    updateElement: batchWrite,
  }
}

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
    if (!enabled) return

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

export function useThrottle<T extends (...args: any[]) => void>(
  callback: T,
  ms: number,
): T {
  const lastRunRef = useRef(0)
  const callbackRef = useRef(callback)

  useEffect(() => {
    callbackRef.current = callback
  }, [callback])

  return useCallback(
    (...args: Parameters<T>) => {
      const nowTime = now()
      if (nowTime - lastRunRef.current >= ms) {
        lastRunRef.current = nowTime
        callbackRef.current(...args)
      }
    },
    [ms],
  ) as T
}

export function useDebounce<T>(value: T, ms: number): T {
  const [debounced, setDebounced] = useState(value)

  useEffect(() => {
    const timer = setTimeout(setDebounced, ms, value)
    return () => clearTimeout(timer)
  }, [value, ms])

  return debounced
}
