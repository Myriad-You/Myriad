/**
 * 元素动画 Hook
 *
 * 用于组件和元素级动画
 * 自动等待页面就绪，支持交错延迟
 */

import type { ElementAnimationOptions } from './types'
import { useCallback, useEffect, useReducer, useRef } from 'react'
import { coordinator } from './coordinator'
import { AnimationPriority, AnimationState } from './types'

interface UseElementAnimationResult {
  /** 是否可以开始动画 */
  canAnimate: boolean
  /** 动画是否正在进行 */
  isAnimating: boolean
  /** 动画完成回调 */
  onComplete: () => void
}

let elementIdCounter = 0

/**
 * 元素动画 Hook
 *
 * @example
 * ```tsx
 * function Card({ index }) {
 *   const { canAnimate, onComplete } = useElementAnimation({
 *     groupId: 'cards',
 *     index,
 *   });
 *
 *   return (
 *     <motion.div
 *       animate={canAnimate ? { opacity: 1 } : { opacity: 0 }}
 *       onAnimationComplete={onComplete}
 *     />
 *   );
 * }
 * ```
 */
export function useElementAnimation(options: ElementAnimationOptions = {}): UseElementAnimationResult {
  const {
    groupId,
    index = 0,
    staggerDelay,
    waitForPage = true,
  } = options

  // 生成稳定的 ID
  const idRef = useRef<string>('')
  if (!idRef.current) {
    idRef.current = `element-${++elementIdCounter}`
  }
  const id = idRef.current

  // 自定义交错将手动计算延迟，以避免协调器重复叠加
  const hasCustomStagger = typeof staggerDelay === 'number'
  const computedDelay = hasCustomStagger
    ? index * (staggerDelay ?? 0)
    : 0
  const effectiveGroupId = hasCustomStagger ? undefined : groupId

  // 使用 ref 存储状态，避免不必要的渲染
  const stateRef = useRef<AnimationState>(AnimationState.WAITING)
  // 🔧 优化：追踪是否已调度
  const scheduledRef = useRef(false)
  const [, forceUpdate] = useReducer(x => x + 1, 0)

  const canAnimate = stateRef.current === AnimationState.READY
    || stateRef.current === AnimationState.RUNNING
  const isAnimating = stateRef.current === AnimationState.RUNNING

  useEffect(() => {
    // 如果不需要等待页面，检查是否可以直接开始
    if (!waitForPage) {
      stateRef.current = AnimationState.READY
      forceUpdate()
      return
    }

    // 🔧 优化：如果已经调度过，跳过重复调度
    if (scheduledRef.current) {
      return
    }
    scheduledRef.current = true

    // 调度动画
    coordinator.schedule({
      id,
      priority: AnimationPriority.ELEMENT,
      groupId: effectiveGroupId,
      index,
      delay: hasCustomStagger ? computedDelay : 0,
    })

    // 订阅状态变化
    const unsubscribe = coordinator.subscribe(id, (state) => {
      const prevState = stateRef.current
      stateRef.current = state

      // 只在状态真正变化时触发渲染
      if (prevState !== state && (
        state === AnimationState.READY
        || state === AnimationState.SKIPPED
      )) {
        forceUpdate()
      }
    })

    return () => {
      unsubscribe()
      scheduledRef.current = false
      if (stateRef.current !== AnimationState.COMPLETED) {
        coordinator.skip(id)
        stateRef.current = AnimationState.SKIPPED
      }
    }
  }, [id, effectiveGroupId, hasCustomStagger, computedDelay, index, waitForPage])

  const onComplete = useCallback(() => {
    coordinator.markCompleted(id)
    stateRef.current = AnimationState.COMPLETED
  }, [id])

  return {
    canAnimate,
    isAnimating,
    onComplete,
  }
}

export default useElementAnimation
