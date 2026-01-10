/**
 * 交错动画 Hook
 *
 * 专门用于列表项等需要交错动画的场景
 * 返回计算后的延迟值，由 CSS 或 framer-motion 使用
 */

import { useEffect, useReducer, useRef } from 'react'
import { coordinator } from './coordinator'
import { AnimationPriority, AnimationState } from './types'

interface UseStaggerAnimationOptions {
  /** 分组ID */
  groupId: string
  /** 在组内的索引 */
  index: number
  /** 基础延迟(ms)，默认使用协调器配置 */
  baseDelay?: number
  /** 是否等待页面就绪 */
  waitForPage?: boolean
}

interface StaggerAnimationResult {
  /** 计算后的延迟(ms) */
  delay: number
  /** 是否可以开始动画 */
  canAnimate: boolean
  /** 动画完成回调 */
  onComplete: () => void
}

let staggerIdCounter = 0

/**
 * 交错动画 Hook
 *
 * @example
 * ```tsx
 * function ListItem({ index }) {
 *   const { delay, canAnimate } = useStaggerAnimation({
 *     groupId: 'list',
 *     index,
 *   });
 *
 *   return (
 *     <motion.div
 *       animate={canAnimate ? { opacity: 1, y: 0 } : { opacity: 0, y: 20 }}
 *       transition={{ delay: delay / 1000 }}
 *     />
 *   );
 * }
 * ```
 *
 * 或使用 CSS 变量：
 * ```tsx
 * function ListItem({ index }) {
 *   const { delay, canAnimate } = useStaggerAnimation({ groupId: 'list', index });
 *
 *   return (
 *     <div
 *       className={canAnimate ? 'animate-in' : ''}
 *       style={{ '--delay': `${delay}ms` }}
 *     />
 *   );
 * }
 * ```
 */
export function useStaggerAnimation(options: UseStaggerAnimationOptions): StaggerAnimationResult {
  const { groupId, index, baseDelay, waitForPage = true } = options

  // 生成稳定的 ID
  const idRef = useRef<string>('')
  if (!idRef.current) {
    idRef.current = `stagger-${groupId}-${++staggerIdCounter}`
  }
  const id = idRef.current

  // 计算延迟
  const delay = coordinator.getStaggerDelay(index, baseDelay)

  // 状态管理
  const stateRef = useRef<AnimationState>(AnimationState.WAITING)
  // 🔧 优化：追踪是否已调度，避免重复调用 schedule
  const scheduledRef = useRef(false)
  const [, forceUpdate] = useReducer(x => x + 1, 0)

  const canAnimate = stateRef.current === AnimationState.READY
    || stateRef.current === AnimationState.RUNNING
    || stateRef.current === AnimationState.COMPLETED

  useEffect(() => {
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

    // 调度动画（协调器内部会处理交错延迟）
    coordinator.schedule({
      id,
      priority: AnimationPriority.ELEMENT,
      groupId,
      index,
      delay: 0, // 延迟由协调器计算
    })

    // 订阅状态变化
    const unsubscribe = coordinator.subscribe(id, (state) => {
      const prevState = stateRef.current
      stateRef.current = state

      if (prevState !== state && state === AnimationState.READY) {
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
  }, [id, groupId, index, waitForPage])

  const onComplete = () => {
    coordinator.markCompleted(id)
    stateRef.current = AnimationState.COMPLETED
  }

  return {
    delay,
    canAnimate,
    onComplete,
  }
}

export default useStaggerAnimation
