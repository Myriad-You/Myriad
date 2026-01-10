/**
 * Tapp 页专用调度器 Hooks
 *
 * Tapp 页功能需求：
 * - Stagger: 卡片列表交错入场动画
 * - Visibility: 切换标签页时暂停/恢复动画
 *
 * @example
 * ```tsx
 * // 在 TappListPage.tsx 中
 * import { useTappScheduler, useTappStagger } from '@hooks/animation/pages/tapp';
 *
 * function TappListPage() {
 *   useTappScheduler();
 *   return <TappGrid />;
 * }
 *
 * function TappCard({ index }) {
 *   const { canAnimate, delay, onComplete } = useTappStagger(index);
 *   return (
 *     <motion.div
 *       initial={{ opacity: 0, y: 10 }}
 *       animate={canAnimate ? { opacity: 1, y: 0 } : { opacity: 0, y: 10 }}
 *       transition={{ delay: delay / 1000 }}
 *       onAnimationComplete={onComplete}
 *     />
 *   );
 * }
 * ```
 */

import { useCallback, useEffect, useReducer, useRef } from 'react'
import { coordinator } from '../coordinator'
import { isPageVisible } from '../core'
import { AnimationPriority, AnimationState } from '../types'

const PAGE_ID = 'tapp'
const STAGGER_GROUP_ID = 'tapp-cards'
const BASE_STAGGER_DELAY = 60 // ms

let staggerIdCounter = 0

// ==================== 页面初始化 ====================

/**
 * Tapp 页调度器初始化
 *
 * 注意：startPage('tapp') 由 useRouteScheduler 统一调用
 */
export function useTappScheduler(): void {
  useEffect(() => {
    return () => {
      // 页面卸载时重置计数器
      staggerIdCounter = 0
    }
  }, [])
}

// ==================== Stagger Animation Hook ====================

interface TappStaggerResult {
  /** 是否可以开始动画 */
  canAnimate: boolean
  /** 延迟时间（毫秒） */
  delay: number
  /** 动画完成回调 */
  onComplete: () => void
}

/**
 * Tapp 卡片交错动画 Hook
 *
 * @param index - 卡片在列表中的索引
 * @param baseDelay - 基础延迟（可选，默认 60ms）
 */
export function useTappStagger(
  index: number,
  baseDelay: number = BASE_STAGGER_DELAY,
): TappStaggerResult {
  // 生成稳定的动画 ID
  const idRef = useRef<string>('')
  if (!idRef.current) {
    idRef.current = `tapp-card-${++staggerIdCounter}`
  }
  const id = idRef.current

  // 计算延迟
  const delay = coordinator.getStaggerDelay(index, baseDelay)

  // 状态管理（使用 ref 避免不必要的渲染）
  const stateRef = useRef<AnimationState>(AnimationState.WAITING)
  const scheduledRef = useRef(false)
  const [, forceUpdate] = useReducer(x => x + 1, 0)

  const canAnimate = stateRef.current === AnimationState.READY
    || stateRef.current === AnimationState.RUNNING
    || stateRef.current === AnimationState.COMPLETED

  // 动画完成回调
  const onComplete = useCallback(() => {
    if (stateRef.current === AnimationState.RUNNING) {
      stateRef.current = AnimationState.COMPLETED
      coordinator.markCompleted(id)
    }
  }, [id])

  useEffect(() => {
    // 避免重复调度
    if (scheduledRef.current)
      return
    scheduledRef.current = true

    // 调度动画
    coordinator.schedule({
      id,
      priority: AnimationPriority.COMPONENT,
      groupId: STAGGER_GROUP_ID,
      index,
      delay,
    })

    // 订阅状态变化
    const unsubscribe = coordinator.subscribe(id, (state) => {
      const prev = stateRef.current
      stateRef.current = state

      // 只在关键状态变化时触发渲染
      if (
        (prev === AnimationState.WAITING && state === AnimationState.READY)
        || (prev === AnimationState.SCHEDULED && state === AnimationState.READY)
        || state === AnimationState.SKIPPED
      ) {
        forceUpdate()
      }
    })

    return () => {
      unsubscribe()
      scheduledRef.current = false
    }
  }, [id, index, delay])

  return { canAnimate, delay, onComplete }
}

// ==================== Visibility Hook ====================

/**
 * Tapp 页面可见性 Hook
 * 用于在页面不可见时暂停动画
 */
export function useTappVisibility(): boolean {
  const [visible, setVisible] = useReducer(() => isPageVisible(), isPageVisible())

  useEffect(() => {
    const handleVisibilityChange = () => {
      setVisible()
    }

    document.addEventListener('visibilitychange', handleVisibilityChange)
    return () => {
      document.removeEventListener('visibilitychange', handleVisibilityChange)
    }
  }, [])

  return visible
}

// ==================== 清理 ====================

export function cleanupTapp(): void {
  staggerIdCounter = 0
}
