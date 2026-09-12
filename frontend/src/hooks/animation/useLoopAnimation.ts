import { useCallback, useEffect, useRef, useState } from 'react'

interface UseLoopAnimationOptions {
  duration?: number
  trigger?: unknown
  enabled?: boolean
}

interface UseLoopAnimationResult {
  isAnimating: boolean
  triggerAnimation: () => void
  stopAnimation: () => void
}

export function useLoopAnimation(
  options: UseLoopAnimationOptions = {},
): UseLoopAnimationResult {
  const { duration = 3000, trigger, enabled = true } = options

  const [isAnimating, setIsAnimating] = useState(false)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const mountedRef = useRef(true)
  const isFirstRender = useRef(true)

  const clearTimer = useCallback(() => {
    if (timerRef.current) {
      clearTimeout(timerRef.current)
      timerRef.current = null
    }
  }, [])

  const startAnimation = useCallback(() => {
    if (!mountedRef.current || !enabled) return

    clearTimer()
    setIsAnimating(true)

    timerRef.current = setTimeout(() => {
      if (mountedRef.current) {
        setIsAnimating(false)
      }
      timerRef.current = null
    }, duration)
  }, [duration, enabled, clearTimer])

  const triggerAnimation = useCallback(() => {
    startAnimation()
  }, [startAnimation])

  const stopAnimation = useCallback(() => {
    clearTimer()
    setIsAnimating(false)
  }, [clearTimer])

  useEffect(() => {
    if (isFirstRender.current) {
      isFirstRender.current = false

      if (enabled && trigger !== undefined) {
        startAnimation()
      }
      return
    }

    if (enabled && trigger !== undefined) {
      startAnimation()
    }
  }, [trigger, enabled, startAnimation])

  useEffect(() => {
    mountedRef.current = true
    return () => {
      mountedRef.current = false
      clearTimer()
    }
  }, [clearTimer])

  return {
    isAnimating,
    triggerAnimation,
    stopAnimation,
  }
}

export default useLoopAnimation
