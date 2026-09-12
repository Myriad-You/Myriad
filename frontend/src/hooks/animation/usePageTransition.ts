import { useCallback, useEffect, useRef } from 'react'
import { coordinator } from './coordinator'
import { AnimationPriority } from './types'

interface UsePageTransitionOptions {

  pageId: string
}

interface PageTransitionResult {

  onEnterComplete: () => void
}

export function usePageTransition({
  pageId,
}: UsePageTransitionOptions): PageTransitionResult {
  const hasStarted = useRef(false)
  const animationId = `page-${pageId}`

  useEffect(() => {
    if (hasStarted.current) return
    hasStarted.current = true

    coordinator.startPageTransition(pageId)

    coordinator.schedule({
      id: animationId,
      priority: AnimationPriority.PAGE,
    })

    return () => {
      hasStarted.current = false
    }
  }, [pageId, animationId])

  const onEnterComplete = useCallback(() => {
    if (!coordinator.completePageTransition(pageId)) return
    coordinator.markCompleted(animationId)
  }, [animationId, pageId])

  return {
    onEnterComplete,
  }
}

// Legacy export.
export const pageTransitionManager = {
  startExit: () => {},
  completeExit: () => {},
  waitForEnter: () => Promise.resolve(),
  checkFirstLoad: () => true,
  reset: () => {},
}

export default usePageTransition
