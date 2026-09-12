import { useCallback, useEffect, useRef } from 'react'
import { isPageVisible, onVisibility } from './core'

interface UseVisibilityIntervalOptions {

  delay: number
  enabled?: boolean

  immediate?: boolean
}

export function useVisibilityInterval(
  callback: () => void,
  options: UseVisibilityIntervalOptions,
) {
  const { delay, enabled = true, immediate = false } = options
  const savedCallback = useRef(callback)
  const timeoutIdRef = useRef<number | null>(null)
  const cancelledRef = useRef(false)

  useEffect(() => {
    savedCallback.current = callback
  }, [callback])

  const clearTimer = useCallback(() => {
    if (timeoutIdRef.current !== null) {
      clearTimeout(timeoutIdRef.current)
      timeoutIdRef.current = null
    }
  }, [])

  const scheduleNext = useCallback(() => {
    if (cancelledRef.current || !isPageVisible()) return

    timeoutIdRef.current = window.setTimeout(() => {
      if (cancelledRef.current || !isPageVisible()) return
      savedCallback.current()
      scheduleNext()
    }, delay)
  }, [delay])

  useEffect(() => {
    if (!enabled) return

    cancelledRef.current = false

    if (immediate && isPageVisible()) {
      savedCallback.current()
    }

    scheduleNext()

    const unsubscribe = onVisibility((isVisible) => {
      if (isVisible) {
        if (timeoutIdRef.current === null && !cancelledRef.current) {
          scheduleNext()
        }
      } else {
        clearTimer()
      }
    })

    return () => {
      cancelledRef.current = true
      clearTimer()
      unsubscribe()
    }
  }, [enabled, delay, immediate, scheduleNext, clearTimer])
}

interface UseVisibilityTimeoutOptions {

  delay: number
  enabled?: boolean
}

export function useVisibilityTimeout(
  callback: () => void,
  options: UseVisibilityTimeoutOptions,
) {
  const { delay, enabled = true } = options
  const savedCallback = useRef(callback)
  const timeoutIdRef = useRef<number | null>(null)
  const startTimeRef = useRef<number>(0)
  const remainingTimeRef = useRef<number>(delay)
  const hasExecutedRef = useRef(false)

  useEffect(() => {
    savedCallback.current = callback
  }, [callback])

  useEffect(() => {
    if (!enabled) return

    hasExecutedRef.current = false
    remainingTimeRef.current = delay

    const startTimer = () => {
      if (hasExecutedRef.current) return

      startTimeRef.current = Date.now()
      timeoutIdRef.current = window.setTimeout(() => {
        if (!hasExecutedRef.current) {
          hasExecutedRef.current = true
          savedCallback.current()
        }
      }, remainingTimeRef.current)
    }

    const pauseTimer = () => {
      if (timeoutIdRef.current !== null) {
        clearTimeout(timeoutIdRef.current)
        timeoutIdRef.current = null

        const elapsed = Date.now() - startTimeRef.current
        remainingTimeRef.current = Math.max(
          0,
          remainingTimeRef.current - elapsed,
        )
      }
    }

    if (isPageVisible()) {
      startTimer()
    }

    const unsubscribe = onVisibility((isVisible) => {
      if (isVisible) {
        startTimer()
      } else {
        pauseTimer()
      }
    })

    return () => {
      if (timeoutIdRef.current !== null) {
        clearTimeout(timeoutIdRef.current)
      }
      unsubscribe()
    }
  }, [enabled, delay])
}

// 规范名在 useSharedEventListener 的 usePageVisibility。
export { usePageVisibility as usePageVisible } from '../useSharedEventListener'
