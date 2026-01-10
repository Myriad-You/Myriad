/**
 * 滚动性能优化 Hook
 *
 * 功能：
 * 1. 滚动时自动添加降级类
 * 2. 滚动结束后恢复
 * 3. 提供滚动状态
 *
 * @module useScrollOptimization
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import { useSharedScroll } from './useSharedEventListener'

interface ScrollOptimizationOptions {
  /** 是否启用 */
  enabled?: boolean
  /** 滚动结束延迟（ms） */
  scrollEndDelay?: number
  /** 降级类名 */
  scrollingClass?: string
  /** 目标元素（默认为 document.body） */
  target?: HTMLElement | null
}

interface ScrollState {
  /** 是否正在滚动 */
  isScrolling: boolean
  /** 滚动方向 */
  direction: 'up' | 'down' | 'none'
  /** 滚动速度（px/s） */
  velocity: number
  /** 当前滚动位置 */
  scrollY: number
}

/**
 * 使用滚动优化
 *
 * 在滚动时自动添加 'is-scrolling' 类到 body，
 * 配合 CSS 可以暂停动画、简化渲染
 */
export function useScrollOptimization(options: ScrollOptimizationOptions = {}): ScrollState {
  const {
    enabled = true,
    scrollEndDelay = 150,
    scrollingClass = 'is-scrolling',
    target = typeof document !== 'undefined' ? document.body : null,
  } = options

  const [state, setState] = useState<ScrollState>({
    isScrolling: false,
    direction: 'none',
    velocity: 0,
    scrollY: typeof window !== 'undefined' ? window.scrollY : 0,
  })

  const lastScrollY = useRef(0)
  const lastScrollTime = useRef(0)
  const scrollEndTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const isScrollingRef = useRef(false)

  const handleScroll = useCallback(() => {
    if (!enabled || !target)
      return

    const now = performance.now()
    const currentScrollY = window.scrollY
    const deltaY = currentScrollY - lastScrollY.current
    const deltaTime = now - lastScrollTime.current

    // 计算速度（px/s）
    const velocity = deltaTime > 0 ? Math.abs(deltaY / deltaTime) * 1000 : 0

    // 确定方向
    const direction: 'up' | 'down' | 'none' = deltaY > 0 ? 'down' : deltaY < 0 ? 'up' : 'none'

    // 更新引用
    lastScrollY.current = currentScrollY
    lastScrollTime.current = now

    // 开始滚动
    if (!isScrollingRef.current) {
      isScrollingRef.current = true
      target.classList.add(scrollingClass)
    }

    // 更新状态
    setState({
      isScrolling: true,
      direction,
      velocity,
      scrollY: currentScrollY,
    })

    // 清除之前的结束定时器
    if (scrollEndTimer.current) {
      clearTimeout(scrollEndTimer.current)
    }

    // 设置滚动结束定时器
    scrollEndTimer.current = setTimeout(() => {
      isScrollingRef.current = false
      target.classList.remove(scrollingClass)
      setState(prev => ({
        ...prev,
        isScrolling: false,
        velocity: 0,
      }))
    }, scrollEndDelay)
  }, [enabled, target, scrollingClass, scrollEndDelay])

  // 使用共享滚动监听器
  useSharedScroll(handleScroll, { enabled })

  // 清理
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

/**
 * 使用快速滚动检测
 * 当滚动速度超过阈值时触发降级
 */
export function useFastScrollDetection(options: {
  velocityThreshold?: number
  onFastScroll?: () => void
  onSlowScroll?: () => void
} = {}): boolean {
  const {
    velocityThreshold = 1500, // px/s
    onFastScroll,
    onSlowScroll,
  } = options

  const [isFastScrolling, setIsFastScrolling] = useState(false)
  const lastScrollY = useRef(0)
  const lastTime = useRef(0)
  const wasFast = useRef(false)

  const handleScroll = useCallback(() => {
    const now = performance.now()
    const currentScrollY = window.scrollY
    const deltaY = Math.abs(currentScrollY - lastScrollY.current)
    const deltaTime = now - lastTime.current

    lastScrollY.current = currentScrollY
    lastTime.current = now

    if (deltaTime <= 0)
      return

    const velocity = (deltaY / deltaTime) * 1000
    const isFast = velocity > velocityThreshold

    if (isFast !== wasFast.current) {
      wasFast.current = isFast
      setIsFastScrolling(isFast)

      if (isFast) {
        onFastScroll?.()
      }
      else {
        onSlowScroll?.()
      }
    }
  }, [velocityThreshold, onFastScroll, onSlowScroll])

  useSharedScroll(handleScroll)

  return isFastScrolling
}

/**
 * 使用滚动方向
 */
export function useScrollDirection(): 'up' | 'down' | 'none' {
  const [direction, setDirection] = useState<'up' | 'down' | 'none'>('none')
  const lastScrollY = useRef(0)

  const handleScroll = useCallback(() => {
    const currentScrollY = window.scrollY
    const deltaY = currentScrollY - lastScrollY.current

    if (Math.abs(deltaY) > 5) { // 忽略微小滚动
      setDirection(deltaY > 0 ? 'down' : 'up')
    }

    lastScrollY.current = currentScrollY
  }, [])

  useSharedScroll(handleScroll)

  return direction
}

/**
 * 使用滚动锁定
 * 在执行某些操作时锁定滚动
 */
export function useScrollLock(): {
  isLocked: boolean
  lock: () => void
  unlock: () => void
} {
  const [isLocked, setIsLocked] = useState(false)
  const originalStyle = useRef<string>('')

  const lock = useCallback(() => {
    if (typeof document === 'undefined')
      return

    originalStyle.current = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    setIsLocked(true)
  }, [])

  const unlock = useCallback(() => {
    if (typeof document === 'undefined')
      return

    document.body.style.overflow = originalStyle.current
    setIsLocked(false)
  }, [])

  // 组件卸载时恢复
  useEffect(() => {
    return () => {
      if (isLocked && typeof document !== 'undefined') {
        document.body.style.overflow = originalStyle.current
      }
    }
  }, [isLocked])

  return { isLocked, lock, unlock }
}

/**
 * 平滑滚动到元素
 * 使用 RAF 实现平滑滚动，避免浏览器默认行为的性能问题
 */
export function smoothScrollTo(
  target: number | HTMLElement,
  options: {
    duration?: number
    easing?: (t: number) => number
    offset?: number
  } = {},
): Promise<void> {
  const {
    duration = 500,
    easing = t => t < 0.5 ? 2 * t * t : -1 + (4 - 2 * t) * t, // easeInOutQuad
    offset = 0,
  } = options

  return new Promise((resolve) => {
    const startY = window.scrollY
    const targetY = typeof target === 'number'
      ? target
      : target.getBoundingClientRect().top + startY
    const deltaY = targetY - startY + offset
    const startTime = performance.now()

    function step(currentTime: number) {
      const elapsed = currentTime - startTime
      const progress = Math.min(elapsed / duration, 1)
      const easedProgress = easing(progress)

      window.scrollTo(0, startY + deltaY * easedProgress)

      if (progress < 1) {
        requestAnimationFrame(step)
      }
      else {
        resolve()
      }
    }

    requestAnimationFrame(step)
  })
}
