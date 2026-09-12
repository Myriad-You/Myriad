import { useCallback, useEffect, useRef, useState } from 'react'
import { useSharedScroll } from './useSharedEventListener'

interface ScrollOptimizationOptions {
  enabled?: boolean
  scrollEndDelay?: number
  scrollingClass?: string
  target?: HTMLElement | null
}

interface ScrollState {
  isScrolling: boolean
  direction: 'up' | 'down' | 'none'
  velocity: number
  scrollY: number
}

export function useScrollOptimization(
  options: ScrollOptimizationOptions = {},
): ScrollState {
  const {
    enabled = true,
    scrollEndDelay = 150,
    scrollingClass = 'is-scrolling',
    target = typeof document !== 'undefined' ? document.body : null,
  } = options

  // 高频滚动数据放 ref；仅 isScrolling 起停时 setState。
  const stateRef = useRef<ScrollState>({
    isScrolling: false,
    direction: 'none',
    velocity: 0,
    scrollY: typeof window !== 'undefined' ? window.scrollY : 0,
  })

  const [state, setState] = useState<ScrollState>(stateRef.current)

  const lastScrollY = useRef(0)
  const lastScrollTime = useRef(0)
  const scrollEndTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const isScrollingRef = useRef(false)

  const handleScroll = useCallback(() => {
    if (!enabled || !target) return

    const now = performance.now()
    const currentScrollY = window.scrollY
    const deltaY = currentScrollY - lastScrollY.current
    const deltaTime = now - lastScrollTime.current

    const velocity = deltaTime > 0 ? Math.abs(deltaY / deltaTime) * 1000 : 0

    const direction: 'up' | 'down' | 'none' =
      deltaY > 0 ? 'down' : deltaY < 0 ? 'up' : 'none'

    lastScrollY.current = currentScrollY
    lastScrollTime.current = now

    stateRef.current.direction = direction
    stateRef.current.velocity = velocity
    stateRef.current.scrollY = currentScrollY

    if (!isScrollingRef.current) {
      isScrollingRef.current = true
      target.classList.add(scrollingClass)
      stateRef.current.isScrolling = true
      setState({ ...stateRef.current })
    }

    if (scrollEndTimer.current) {
      clearTimeout(scrollEndTimer.current)
    }

    scrollEndTimer.current = setTimeout(() => {
      isScrollingRef.current = false
      target.classList.remove(scrollingClass)
      stateRef.current.isScrolling = false
      stateRef.current.velocity = 0
      setState({ ...stateRef.current })
    }, scrollEndDelay)
  }, [enabled, target, scrollingClass, scrollEndDelay])

  useSharedScroll(handleScroll, { enabled })

  useEffect(() => {
    return () => {
      if (scrollEndTimer.current) {
        clearTimeout(scrollEndTimer.current)
      }
      if (target && isScrollingRef.current) {
        target.classList.remove(scrollingClass)
      }
    }
  }, [target, scrollingClass])

  return state
}
