import { useCallback, useEffect, useRef } from 'react'
import { Feature, hasFeature } from '../pageFeatures'

type SimplePageId = 'config' | 'login' | 'setup'

export function useSimplePageScheduler(_pageId: SimplePageId): void {

}

// startPage 由 useRouteScheduler 统一调用。
export function useConfigScheduler(): void {

}

export function useLoginScheduler(): void {

}

export function useSetupScheduler(): void {

}

export function useDetailsScheduler(): void {

}

export function useSimpleTimeout(
  callback: () => void,
  delay: number | null,
  pageId: SimplePageId = 'config',
): void {
  const savedCallback = useRef(callback)

  useEffect(() => {
    savedCallback.current = callback
  }, [callback])

  useEffect(() => {
    if (!hasFeature(pageId, Feature.Timeout) || delay === null) {
      return
    }

    const id = setTimeout(() => savedCallback.current(), delay)
    return () => clearTimeout(id)
  }, [delay, pageId])
}

export function useSimpleDebounce<T extends (...args: any[]) => void>(
  callback: T,
  delay: number,
  pageId: SimplePageId = 'config',
): T {
  const timeoutRef = useRef<number | null>(null)
  const savedCallback = useRef(callback)

  useEffect(() => {
    savedCallback.current = callback
  }, [callback])

  const debounced = useCallback(
    (...args: any[]) => {
      if (!hasFeature(pageId, Feature.Timeout)) {
        savedCallback.current(...args)
        return
      }

      if (timeoutRef.current !== null) {
        clearTimeout(timeoutRef.current)
      }

      timeoutRef.current = window.setTimeout(() => {
        timeoutRef.current = null
        savedCallback.current(...args)
      }, delay)
    },
    [delay, pageId],
  ) as T

  useEffect(() => {
    return () => {
      if (timeoutRef.current !== null) {
        clearTimeout(timeoutRef.current)
      }
    }
  }, [])

  return debounced
}

export function useSimpleThrottle<T extends (...args: any[]) => void>(
  callback: T,
  delay: number,
  pageId: SimplePageId = 'config',
): T {
  const lastRun = useRef(0)
  const timeoutRef = useRef<number | null>(null)
  const savedCallback = useRef(callback)

  useEffect(() => {
    savedCallback.current = callback
  }, [callback])

  const throttled = useCallback(
    (...args: any[]) => {
      if (!hasFeature(pageId, Feature.Timeout)) {
        savedCallback.current(...args)
        return
      }

      const now = Date.now()
      const remaining = delay - (now - lastRun.current)

      if (remaining <= 0) {
        lastRun.current = now
        savedCallback.current(...args)
      } else if (timeoutRef.current === null) {
        timeoutRef.current = window.setTimeout(() => {
          lastRun.current = Date.now()
          timeoutRef.current = null
          savedCallback.current(...args)
        }, remaining)
      }
    },
    [delay, pageId],
  ) as T

  useEffect(() => {
    return () => {
      if (timeoutRef.current !== null) {
        clearTimeout(timeoutRef.current)
      }
    }
  }, [])

  return throttled
}
