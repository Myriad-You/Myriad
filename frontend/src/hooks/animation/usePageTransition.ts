/**
 * 页面过渡 Hook - 简化版
 *
 * 页面级进入/退出动画现在由 App.tsx 中的 PageWrapper 处理
 * 此 Hook 只负责通知协调器页面状态
 */

import { useCallback, useEffect, useRef } from 'react'
import { coordinator } from './coordinator'
import { AnimationPriority } from './types'

interface UsePageTransitionOptions {
  /** 页面唯一标识 */
  pageId: string
}

interface PageTransitionResult {
  /** 进入动画完成回调 */
  onEnterComplete: () => void
}

/**
 * 页面过渡 Hook
 *
 * @example
 * ```tsx
 * function AnimatedView({ children, id }) {
 *   const { onEnterComplete } = usePageTransition({ pageId: id });
 *
 *   useEffect(() => {
 *     // 页面加载完成后通知
 *     onEnterComplete();
 *   }, []);
 *
 *   return <div>{children}</div>;
 * }
 * ```
 */
export function usePageTransition({
  pageId,
}: UsePageTransitionOptions): PageTransitionResult {
  const hasStarted = useRef(false)
  const animationId = `page-${pageId}`

  useEffect(() => {
    if (hasStarted.current)
      return
    hasStarted.current = true

    // 通知协调器开始页面过渡
    coordinator.startPageTransition(pageId)

    // 调度页面动画
    coordinator.schedule({
      id: animationId,
      priority: AnimationPriority.PAGE,
    })

    return () => {
      hasStarted.current = false
    }
  }, [pageId, animationId])

  const onEnterComplete = useCallback(() => {
    coordinator.markCompleted(animationId)
    coordinator.completePageTransition()
  }, [animationId])

  return {
    onEnterComplete,
  }
}

// 保留 pageTransitionManager 导出以保持向后兼容
export const pageTransitionManager = {
  startExit: () => {},
  completeExit: () => {},
  waitForEnter: () => Promise.resolve(),
  checkFirstLoad: () => true,
  reset: () => {},
}

export default usePageTransition
