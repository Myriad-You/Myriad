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
export function useScrollOptimization(
  options: ScrollOptimizationOptions = {},
): ScrollState {
  const {
    enabled = true,
    scrollEndDelay = 150,
    scrollingClass = 'is-scrolling',
    target = typeof document !== 'undefined' ? document.body : null,
  } = options

  // 使用 ref 存储高频变化的滚动数据，避免每帧 setState 导致消费者重渲染
  // 仅在 isScrolling 状态切换时才触发 React 更新（开始/结束各一次）
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

    // 计算速度（px/s）
    const velocity = deltaTime > 0 ? Math.abs(deltaY / deltaTime) * 1000 : 0

    // 确定方向
    const direction: 'up' | 'down' | 'none' =
      deltaY > 0 ? 'down' : deltaY < 0 ? 'up' : 'none'

    // 更新引用
    lastScrollY.current = currentScrollY
    lastScrollTime.current = now

    // 始终更新 ref（无渲染开销）
    stateRef.current.direction = direction
    stateRef.current.velocity = velocity
    stateRef.current.scrollY = currentScrollY

    // 开始滚动 — 仅在状态切换时 setState
    if (!isScrollingRef.current) {
      isScrollingRef.current = true
      target.classList.add(scrollingClass)
      stateRef.current.isScrolling = true
      setState({ ...stateRef.current })
    }

    // 清除之前的结束定时器
    if (scrollEndTimer.current) {
      clearTimeout(scrollEndTimer.current)
    }

    // 设置滚动结束定时器
    scrollEndTimer.current = setTimeout(() => {
      isScrollingRef.current = false
      target.classList.remove(scrollingClass)
      stateRef.current.isScrolling = false
      stateRef.current.velocity = 0
      setState({ ...stateRef.current })
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
